//! The user's sites (B3): `sites.json` (origins, names, the cached
//! well-known document, the signed-in account and the challenges taken
//! from each; no secrets), one [`Session`] per site and the connect log
//! (B5), all in the app's data folder, never in `Documents`.

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
use crate::discovery::{self, WellKnown};
use crate::http::Http;
use crate::origin::Origin;
use crate::session::{Log, Session};
use crate::store::SecretStore;
use crate::{jsonl, lock, now_ms};

const FILE: &str = "sites.json";
const LOG: &str = "connect.log.jsonl";
/// The well-known document is fetched again after this long (A1).
const WELL_KNOWN_TTL_MS: i64 = 24 * 3600 * 1000;

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
    allow_local: bool,
    records: Mutex<Vec<SiteRecord>>,
    sessions: Mutex<HashMap<String, Arc<Session>>>,
    signing_in: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl Sites {
    pub fn open(
        dir: PathBuf,
        http: Arc<Http>,
        store: Arc<dyn SecretStore>,
        allow_local: bool,
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
            allow_local,
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

    /// Adds the site the user typed: its address checked (A1), its
    /// well-known document fetched and checked. Adding it again refreshes it.
    pub fn add(self: &Arc<Self>, input: &str) -> Result<SiteRecord, String> {
        let origin = Origin::parse(input, self.allow_local)?;
        let well_known = discovery::discover(&self.http, &origin)?;
        let old = self.get(origin.as_str());
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
        lock(&self.sessions).remove(origin.as_str());
        self.log(json!({ "event": "site_added", "origin": origin }));
        Ok(record)
    }

    /// Fetches the well-known document again when the cached one is a day
    /// old; a failure keeps the cached one.
    pub fn freshen(self: &Arc<Self>, origin: &str) {
        let stale = self
            .get(origin)
            .is_some_and(|r| now_ms() - r.fetched_at_ms > WELL_KNOWN_TTL_MS);
        if stale && let Err(e) = self.add(origin) {
            self.log(json!({ "event": "well_known", "origin": origin, "error": e }));
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

    /// Signs out and forgets the site.
    pub fn remove(self: &Arc<Self>, origin: &str) -> Result<(), String> {
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

    pub fn sites(dir: &std::path::Path, store: Arc<dyn SecretStore>) -> Arc<Sites> {
        Arc::new(
            Sites::open(
                dir.join("connect"),
                Arc::new(Http::new("0.2.0")),
                store,
                true,
            )
            .unwrap(),
        )
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
