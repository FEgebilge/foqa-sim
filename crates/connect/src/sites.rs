//! The user's sites (B3): `sites.json` (origins, names, the cached
//! well-known document, the signed-in account and the challenges taken
//! from each; no secrets), one [`Session`] per site and the connect log
//! (B5), all in the app's data folder, never in `Documents`. The app may
//! ship with one site built in; others only behind its developer switch
//! ([`Policy`]).

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::thread;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::api::{Error, User};
use crate::auth::{self, AuthError, DeviceCode};
use crate::copy::{self, Ctx};
use crate::discovery::{self, Failed, WellKnown};
use crate::http::Http;
use crate::origin::Origin;
use crate::session::{Log, Session};
use crate::store::SecretStore;
use crate::{jsonl, lock, now_ms};

const FILE: &str = "sites.json";
const LOG: &str = "connect.log.jsonl";
/// The well-known document is fetched again after this long (A1).
const WELL_KNOWN_TTL_MS: i64 = 24 * 3600 * 1000;
/// The built-in site's document, while it has none, is asked for again
/// after this long (at once on sign-in).
const BUILT_IN_RETRY_MS: i64 = 60 * 1000;

/// The site the app ships with: listed before its well-known document
/// could be fetched, and never removed.
#[derive(Debug, Clone)]
pub struct BuiltIn {
    pub origin: Origin,
    pub name: String,
}

/// Which sites the app may have.
#[derive(Debug, Clone, Default)]
pub struct Policy {
    pub built_in: Option<BuiltIn>,
    /// Other sites may be added, by address or by a link (the developer
    /// switch).
    pub others: bool,
    /// "Allow local sites" (A1).
    pub allow_local: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SiteRecord {
    pub origin: Origin,
    pub well_known: WellKnown,
    pub fetched_at_ms: i64,
    /// The account signed in last (kept after sign-out, to tell a
    /// different account apart on the next sign-in).
    pub user: Option<User>,
    #[serde(default)]
    pub challenges: Vec<Linked>,
}

impl SiteRecord {
    /// `<name> (<ascii host>)`, or the host alone.
    pub fn label(&self) -> String {
        match self.well_known.name.as_str() {
            "" => self.origin.host().to_owned(),
            name => format!("{name} ({})", self.origin.host()),
        }
    }
}

/// A challenge taken from a site into the app's library under `key`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Linked {
    pub key: String,
    pub id: String,
    pub version: u32,
}

pub enum Flow<'a> {
    /// The system browser and the loopback redirect; `open` opens a URL.
    Browser(&'a dyn Fn(&str) -> Result<(), String>),
    /// The device flow; `show` gets the code to show (and is given the
    /// page to open, when it is on the site's origin).
    Device(&'a dyn Fn(&DeviceCode, Option<&str>)),
}

pub struct Sites {
    dir: PathBuf,
    http: Arc<Http>,
    store: Arc<dyn SecretStore>,
    policy: Policy,
    /// When the built-in site's document was last missed, and why.
    tried: Mutex<Option<(i64, String)>>,
    records: Mutex<Vec<SiteRecord>>,
    sessions: Mutex<HashMap<String, Arc<Session>>>,
    signing_in: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl Sites {
    pub fn open(
        dir: PathBuf,
        http: Arc<Http>,
        store: Arc<dyn SecretStore>,
        policy: Policy,
    ) -> io::Result<Self> {
        fs::create_dir_all(&dir)?;
        let records = fs::read(dir.join(FILE))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Ok(Self {
            dir,
            http,
            store,
            policy,
            tried: Mutex::new(None),
            records: Mutex::new(records),
            sessions: Mutex::new(HashMap::new()),
            signing_in: Mutex::new(HashMap::new()),
        })
    }

    pub fn list(&self) -> Vec<SiteRecord> {
        lock(&self.records).clone()
    }

    pub fn get(&self, origin: &str) -> Option<SiteRecord> {
        lock(&self.records)
            .iter()
            .find(|r| r.origin.as_str() == origin)
            .cloned()
    }

    fn get_or(&self, origin: &str) -> Result<SiteRecord, String> {
        self.get(origin)
            .ok_or_else(|| format!("{origin} isn't one of your sites."))
    }

    fn save(&self, edit: impl FnOnce(&mut Vec<SiteRecord>)) -> Result<(), String> {
        let mut records = lock(&self.records);
        edit(&mut records);
        let part = self.dir.join(format!("{FILE}.part"));
        let bytes = serde_json::to_vec_pretty(&*records).map_err(|e| e.to_string())?;
        fs::write(&part, bytes).map_err(|e| e.to_string())?;
        fs::rename(part, self.dir.join(FILE)).map_err(|e| e.to_string())
    }

    /// One line of the connect log (rotated at 1 MiB, 3 files kept).
    pub fn log(&self, fields: Value) {
        let path = self.dir.join(LOG);
        jsonl::rotate(&path, 1024 * 1024, 3);
        if let Err(e) = jsonl::append(&path, fields) {
            eprintln!("connect log: {e}");
        }
    }

    fn logger(self: &Arc<Self>) -> Log {
        let sites = Arc::downgrade(self);
        Arc::new(move |v| {
            if let Some(s) = sites.upgrade() {
                s.log(v);
            }
        })
    }

    pub fn built_in(&self) -> Option<&BuiltIn> {
        self.policy.built_in.as_ref()
    }

    pub fn is_built_in(&self, origin: &str) -> bool {
        self.built_in().is_some_and(|b| b.origin.as_str() == origin)
    }

    /// Whether `origin` may be added: the built-in site, or any with the
    /// developer switch.
    pub fn allows(&self, origin: &Origin) -> bool {
        self.policy.others || self.is_built_in(origin.as_str())
    }

    /// Why the built-in site can't be used yet; `None` once its document
    /// is kept (or with none built in). It is fetched again on `force` or
    /// a minute after the last try. A missing document is no error to
    /// log: challenges just aren't open there yet.
    pub fn built_in_closed(self: &Arc<Self>, force: bool) -> Option<String> {
        let b = self.built_in()?;
        if self.get(b.origin.as_str()).is_some() {
            return None;
        }
        let mut tried = lock(&self.tried);
        if let Some((at, why)) = &*tried
            && !force
            && now_ms() - at < BUILT_IN_RETRY_MS
        {
            return Some(why.clone());
        }
        let why = match self.fetch(b.origin.clone()) {
            Ok(_) => {
                *tried = None;
                return None;
            }
            Err(f) if f.missing => format!("Challenges aren't open on {} yet.", b.name),
            Err(f) => f.text,
        };
        *tried = Some((now_ms(), why.clone()));
        Some(why)
    }

    /// Adds the site the user typed: its address checked (A1) and allowed
    /// ([`Policy`]), its well-known document fetched and checked. Adding it
    /// again refreshes it.
    pub fn add(self: &Arc<Self>, input: &str) -> Result<SiteRecord, String> {
        let origin = Origin::parse(input, self.policy.allow_local)?;
        if !self.allows(&origin) {
            let name = self.built_in().map_or("its own site", |b| b.name.as_str());
            return Err(format!("FOQA-Sim connects to {name} only."));
        }
        Ok(self.fetch(origin)?)
    }

    /// Fetches and keeps `origin`'s well-known document.
    fn fetch(self: &Arc<Self>, origin: Origin) -> Result<SiteRecord, Failed> {
        let well_known = discovery::discover(&self.http, &origin)?;
        let old = self.get(origin.as_str());
        // A refreshed document keeps the session (its tokens, its single
        // refresh) unless the API moved.
        let moved = old
            .as_ref()
            .is_none_or(|r| r.well_known.api() != well_known.api());
        let record = SiteRecord {
            origin: origin.clone(),
            well_known,
            fetched_at_ms: now_ms(),
            user: old.as_ref().and_then(|r| r.user.clone()),
            challenges: old.map(|r| r.challenges).unwrap_or_default(),
        };
        let kept = record.clone();
        self.save(|all| {
            all.retain(|r| r.origin != origin);
            all.push(kept);
        })?;
        if moved {
            lock(&self.sessions).remove(origin.as_str());
        }
        self.log(json!({ "event": "site_added", "origin": origin }));
        Ok(record)
    }

    /// Fetches the well-known document again when the cached one is a day
    /// old; a failure keeps the cached one.
    pub fn freshen(self: &Arc<Self>, origin: &str) {
        let stale = self
            .get(origin)
            .filter(|r| now_ms() - r.fetched_at_ms > WELL_KNOWN_TTL_MS);
        if let Some(r) = stale
            && let Err(e) = self.fetch(r.origin)
        {
            self.log(json!({ "event": "well_known", "origin": origin, "error": e.text }));
        }
    }

    /// The site's session; `None` for a site not added.
    pub fn session(self: &Arc<Self>, origin: &str) -> Option<Arc<Session>> {
        let record = self.get(origin)?;
        let mut sessions = lock(&self.sessions);
        let session = sessions.entry(origin.to_owned()).or_insert_with(|| {
            Arc::new(Session::new(
                Arc::clone(&self.http),
                Arc::clone(&self.store),
                origin,
                record.well_known.api(),
                self.logger(),
            ))
        });
        Some(Arc::clone(session))
    }

    pub fn signed_in(self: &Arc<Self>, origin: &str) -> bool {
        self.session(origin).is_some_and(|s| s.has_grant())
    }

    /// Signs in to `origin` and returns the account. Errors are the app's
    /// own words (A6).
    pub fn sign_in(self: &Arc<Self>, origin: &str, flow: Flow) -> Result<User, String> {
        if self.is_built_in(origin)
            && let Some(why) = self.built_in_closed(true)
        {
            return Err(why);
        }
        let record = self.get_or(origin)?;
        let session = self.session(origin).ok_or("no site")?;
        let cancel = Arc::new(AtomicBool::new(false));
        lock(&self.signing_in).insert(origin.into(), Arc::clone(&cancel));
        let (name, tokens) = match flow {
            Flow::Browser(open) => {
                let authorize = &record.well_known.auth.authorize;
                (
                    "browser",
                    auth::sign_in(session.http(), authorize, session.api(), open, &cancel),
                )
            }
            Flow::Device(show) => ("device", self.device(&record, &session, show, &cancel)),
        };
        lock(&self.signing_in).remove(origin);
        let outcome =
            |o: &str| json!({ "event": "sign_in", "origin": origin, "flow": name, "outcome": o });
        let ctx = Ctx {
            site: record.origin.host(),
            upload_by: None,
            max_bytes: record.well_known.max_upload(),
        };
        let tokens = match tokens {
            Ok(t) => t,
            Err(e) => {
                let (code, text) = match &e {
                    AuthError::Denied(code) => (code.clone(), copy::sign_in_failed().to_owned()),
                    AuthError::TimedOut => {
                        ("timeout".into(), "Sign-in timed out. Try again.".into())
                    }
                    AuthError::Cancelled => ("cancelled".into(), "Sign-in cancelled.".into()),
                    AuthError::Local(m) => {
                        ("local".into(), format!("Couldn't start the sign-in: {m}"))
                    }
                    AuthError::Api(Error::Api(api)) if api.http == 400 => {
                        (api.code.clone(), copy::sign_in_failed().to_owned())
                    }
                    AuthError::Api(e) => (e.code().to_owned(), copy::error(e, ctx)),
                };
                self.log(outcome(&code));
                return Err(text);
            }
        };
        session.accept(&tokens).map_err(|e| copy::error(&e, ctx))?;
        let user = match tokens.user {
            Some(u) => u,
            None => session.me().map_err(|e| copy::error(&e, ctx))?,
        };
        let kept = user.clone();
        self.save(|all| {
            if let Some(r) = all.iter_mut().find(|r| r.origin.as_str() == origin) {
                r.user = Some(kept);
            }
        })?;
        self.log(outcome("ok"));
        Ok(user)
    }

    fn device(
        &self,
        record: &SiteRecord,
        session: &Session,
        show: &dyn Fn(&DeviceCode, Option<&str>),
        cancel: &AtomicBool,
    ) -> Result<crate::api::TokenResponse, AuthError> {
        let code = auth::device_start(session.http(), session.api())?;
        let page = code
            .verification_uri_complete
            .as_deref()
            .unwrap_or(&code.verification_uri);
        show(&code, record.origin.owns(page).then_some(page));
        auth::device_poll(session.http(), session.api(), &code, &thread::sleep, cancel)
    }

    /// Stops a sign-in waiting for the browser or the device code.
    pub fn cancel_sign_in(&self, origin: &str) {
        if let Some(c) = lock(&self.signing_in).get(origin) {
            c.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    /// Revokes the grant on the site and deletes the credential; the
    /// grant is forgotten here even when the site can't be reached.
    pub fn sign_out(self: &Arc<Self>, origin: &str) -> Result<(), String> {
        let session = self
            .session(origin)
            .ok_or_else(|| format!("{origin} isn't one of your sites."))?;
        let revoked = session.revoke();
        self.log(json!({ "event": "revoke", "origin": origin, "outcome": revoked.as_ref().map_or_else(|e| e.code().to_owned(), |_| "ok".into()) }));
        Ok(())
    }

    /// Signs out and forgets the site; the built-in one stays.
    pub fn remove(self: &Arc<Self>, origin: &str) -> Result<(), String> {
        if let Some(b) = self.built_in().filter(|b| b.origin.as_str() == origin) {
            return Err(format!("{} comes with FOQA-Sim; sign out instead.", b.name));
        }
        self.sign_out(origin)?;
        self.save(|all| all.retain(|r| r.origin.as_str() != origin))?;
        lock(&self.sessions).remove(origin);
        self.log(json!({ "event": "site_removed", "origin": origin }));
        Ok(())
    }

    /// Notes that library entry `key` is challenge `id` v`version` of `origin`.
    pub fn link(&self, origin: &str, key: &str, id: &str, version: u32) -> Result<(), String> {
        self.get_or(origin)?;
        let linked = Linked {
            key: key.into(),
            id: id.into(),
            version,
        };
        self.save(|all| {
            for r in all.iter_mut() {
                r.challenges.retain(|l| l.key != key);
            }
            if let Some(r) = all.iter_mut().find(|r| r.origin.as_str() == origin) {
                r.challenges.push(linked);
            }
        })
    }

    /// The site a library entry came from.
    pub fn source(&self, key: &str) -> Option<(SiteRecord, Linked)> {
        lock(&self.records).iter().find_map(|r| {
            let l = r.challenges.iter().find(|l| l.key == key)?;
            Some((r.clone(), l.clone()))
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::mock::{Mock, Reply, tokens, well_known};
    use crate::store::MemoryStore;

    /// Any local site, none built in (the developer switch).
    pub fn sites(dir: &std::path::Path, store: Arc<dyn SecretStore>) -> Arc<Sites> {
        let policy = Policy {
            built_in: None,
            others: true,
            allow_local: true,
        };
        with(dir, store, policy)
    }

    fn with(dir: &std::path::Path, store: Arc<dyn SecretStore>, policy: Policy) -> Arc<Sites> {
        let http = Arc::new(Http::new("0.2.0"));
        Arc::new(Sites::open(dir.join("connect"), http, store, policy).unwrap())
    }

    #[test]
    fn a_site_is_added_signed_in_by_device_code_signed_out_and_removed() {
        let mock = Mock::start(|req, base| match req.path.as_str() {
            discovery::PATH => Reply::json(200, well_known(base)),
            "/api/foqa/v1/device/code" => Reply::json(
                200,
                json!({
                    "device_code": "dc", "user_code": "ABCD-EFGH", "verification_uri": format!("{base}/#/device"),
                    "expires_in": 60, "interval": 0
                }),
            ),
            "/api/foqa/v1/token" => Reply::json(200, tokens(1)),
            "/api/foqa/v1/token/revoke" => Reply::raw(204, "application/json", vec![]),
            _ => Reply::error(404, "not_found"),
        });
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(MemoryStore::default());
        let s = sites(tmp.path(), store.clone());
        assert!(s.add("https://93.184.216.34").is_err());
        let r = s.add(&mock.base).unwrap();
        assert_eq!(r.label(), format!("TestSite ({})", r.origin.host()));
        let origin = r.origin.as_str().to_owned();
        assert!(!s.signed_in(&origin));
        let shown = Mutex::new(None);
        let user = s
            .sign_in(
                &origin,
                Flow::Device(&|c, page| {
                    *lock(&shown) = Some((c.user_code.clone(), page.map(str::to_owned)));
                }),
            )
            .unwrap();
        assert_eq!(user.handle.as_deref(), Some("ege"));
        let (code, page) = lock(&shown).clone().unwrap();
        assert_eq!(code, "ABCD-EFGH");
        assert!(
            page.unwrap().ends_with("/#/device"),
            "on the site's origin, so it may be opened"
        );
        assert!(s.signed_in(&origin));
        assert_eq!(store.get(&origin).unwrap().as_deref(), Some("refresh-1"));

        // Reopened from disk: the site and its account are remembered.
        let again = sites(tmp.path(), store.clone());
        assert_eq!(again.get(&origin).unwrap().user.unwrap().id, "u1");
        assert!(
            again.signed_in(&origin),
            "the refresh token is in the store"
        );

        s.link(&origin, "ltfj-06-v1", "ltfj-06", 1).unwrap();
        assert_eq!(s.source("ltfj-06-v1").unwrap().1.id, "ltfj-06");
        s.sign_out(&origin).unwrap();
        assert!(!s.signed_in(&origin) && store.get(&origin).unwrap().is_none());
        s.remove(&origin).unwrap();
        assert!(s.list().is_empty() && s.source("ltfj-06-v1").is_none());
        let log: Vec<String> = jsonl::read(&tmp.path().join("connect").join(LOG))
            .iter()
            .map(|l| l["event"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(
            log,
            ["site_added", "sign_in", "revoke", "revoke", "site_removed"]
        );
        let text = fs::read_to_string(tmp.path().join("connect").join(LOG)).unwrap()
            + &fs::read_to_string(tmp.path().join("connect").join(FILE)).unwrap();
        assert!(
            !text.contains("refresh-1") && !text.contains("access-1"),
            "no secret on disk"
        );
    }

    #[test]
    fn a_day_old_document_is_fetched_again_without_dropping_the_session() {
        let mock = Mock::start(|req, base| match req.path.as_str() {
            discovery::PATH => Reply::json(200, well_known(base)),
            _ => Reply::error(404, "not_found"),
        });
        let tmp = tempfile::tempdir().unwrap();
        // The refresh token lives in memory only: a new session would lose it.
        let s = sites(tmp.path(), Arc::new(MemoryStore::broken()));
        let origin = s.add(&mock.base).unwrap().origin.as_str().to_owned();
        let session = s.session(&origin).unwrap();
        session
            .accept(&serde_json::from_value(tokens(1)).unwrap())
            .unwrap();
        s.save(|all| all[0].fetched_at_ms = 0).unwrap();
        s.freshen(&origin);
        assert_eq!(mock.hits(discovery::PATH), 2, "fetched again");
        assert!(s.get(&origin).unwrap().fetched_at_ms > 0);
        assert!(Arc::ptr_eq(&session, &s.session(&origin).unwrap()));
        assert!(s.signed_in(&origin));
    }

    #[test]
    fn the_built_in_site_is_listed_before_its_document_and_never_removed() {
        let open = Arc::new(AtomicBool::new(false));
        let gate = Arc::clone(&open);
        let mock = Mock::start(move |req, base| match req.path.as_str() {
            discovery::PATH if gate.load(std::sync::atomic::Ordering::SeqCst) => {
                Reply::json(200, well_known(base))
            }
            _ => Reply::error(404, "not_found"),
        });
        let tmp = tempfile::tempdir().unwrap();
        let origin = Origin::parse(&mock.base, true).unwrap();
        let policy = Policy {
            built_in: Some(BuiltIn {
                origin: origin.clone(),
                name: "Pilot Aptitude".into(),
            }),
            others: false,
            allow_local: true,
        };
        let s = with(tmp.path(), Arc::new(MemoryStore::default()), policy);
        let o = origin.as_str();
        let closed = "Challenges aren't open on Pilot Aptitude yet.";
        assert_eq!(s.built_in_closed(false).as_deref(), Some(closed));
        assert_eq!(s.built_in_closed(false).as_deref(), Some(closed));
        assert_eq!(
            mock.hits(discovery::PATH),
            1,
            "not asked again within a minute"
        );
        assert!(s.list().is_empty() && s.remove(o).is_err());
        let refused = s.sign_in(o, Flow::Browser(&|_| Ok(())));
        assert_eq!(refused.unwrap_err(), closed);
        assert_eq!(mock.hits(discovery::PATH), 2, "sign-in asks again at once");
        assert!(
            !fs::read_to_string(tmp.path().join("connect").join(LOG))
                .unwrap_or_default()
                .contains("well_known"),
            "a missing document is no error in the log"
        );

        // Challenges open on the site: it is kept like an added one.
        open.store(true, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(s.built_in_closed(true), None);
        assert_eq!(
            s.get(o).unwrap().label(),
            format!("TestSite ({})", origin.host())
        );
        assert_eq!(s.built_in_closed(false), None);
        assert_eq!(mock.hits(discovery::PATH), 3);
        assert!(s.remove(o).unwrap_err().contains("sign out instead"));
        assert!(s.get(o).is_some());
    }

    #[test]
    fn other_sites_need_the_developer_switch() {
        let mock = Mock::start(|req, base| match req.path.as_str() {
            discovery::PATH => Reply::json(200, well_known(base)),
            _ => Reply::error(404, "not_found"),
        });
        let tmp = tempfile::tempdir().unwrap();
        let built_in = BuiltIn {
            origin: Origin::parse("https://pilotaptitude.example", false).unwrap(),
            name: "Pilot Aptitude".into(),
        };
        let other = Origin::parse(&mock.base, true).unwrap();
        let policy = |others| Policy {
            built_in: Some(built_in.clone()),
            others,
            allow_local: true,
        };
        let store: Arc<dyn SecretStore> = Arc::new(MemoryStore::default());
        let off = with(tmp.path(), Arc::clone(&store), policy(false));
        assert!(!off.allows(&other) && off.allows(&built_in.origin));
        assert_eq!(
            off.add(&mock.base).unwrap_err(),
            "FOQA-Sim connects to Pilot Aptitude only."
        );
        assert_eq!(mock.hits(discovery::PATH), 0, "nothing fetched");
        let on = with(tmp.path(), store, policy(true));
        assert!(on.allows(&other));
        assert!(on.add(&mock.base).is_ok());
    }

    #[test]
    fn a_denied_sign_in_reads_in_plain_words() {
        let mock = Mock::start(|req, base| match req.path.as_str() {
            discovery::PATH => Reply::json(200, well_known(base)),
            "/api/foqa/v1/device/code" => Reply::json(
                200,
                json!({
                    "device_code": "dc", "user_code": "X", "verification_uri": "https://elsewhere.example/device",
                    "expires_in": 60, "interval": 0
                }),
            ),
            _ => Reply::error(400, "access_denied"),
        });
        let tmp = tempfile::tempdir().unwrap();
        let s = sites(tmp.path(), Arc::new(MemoryStore::default()));
        let origin = s.add(&mock.base).unwrap().origin.as_str().to_owned();
        let page = Mutex::new(Some(String::new()));
        let err = s.sign_in(
            &origin,
            Flow::Device(&|_, p| *lock(&page) = p.map(str::to_owned)),
        );
        assert_eq!(err.unwrap_err(), copy::sign_in_failed());
        assert_eq!(*lock(&page), None, "a page on another origin is not opened");
    }
}
