//! The upload queue (B6): issue before placing, then, once the core is
//! done, recording → client result → polling, one attempt at a time, with
//! backoff (5 s, 30 s, 2 min, 10 min, then hourly; `Retry-After` wins)
//! until the local upload deadline. State lives in the sidecars, so the
//! queue survives restarts; offline it simply stays queued.
//!
//! What is uploaded: only the attempt's slice `slice.fqz` (A5), which the
//! core cuts from the flight by the attempt's marker; an attempt without
//! one is refused, never replaced by the whole flight.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::api::{AttemptStatus, Error};
use crate::copy::{self, Ctx, Retry};
use crate::sidecar::{self, Sidecar, SiteState};
use crate::sites::{SiteRecord, Sites};

/// The slice the core cuts into the attempt folder (B5).
pub const SLICE: &str = "slice.fqz";
/// The core's local grade, sent as the display-only client result.
const RESULT: &str = "result.local.json";
/// A compact flight's magic (format.md); the site refuses anything else.
const MAGIC: &[u8] = b"FQJC";
const BACKOFF_S: [i64; 4] = [5, 30, 120, 600];
const HOUR_S: i64 = 3600;
/// While the flight is still open, look again this often.
const NO_RECORDING_S: i64 = 30;

type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

pub struct Uploader {
    sites: Arc<Sites>,
    attempts: PathBuf,
    changed: Box<dyn Fn(&str) + Send + Sync>,
    now: Clock,
}

/// An attempt's site part, as the Challenges screen shows it.
#[derive(Debug, Clone, Serialize)]
pub struct View {
    /// The local attempt id.
    pub id: String,
    pub origin: String,
    pub site: String,
    pub state: SiteState,
    pub text: String,
    pub detail: Option<String>,
    pub status: Option<AttemptStatus>,
    /// The site's page for the attempt.
    pub url: Option<String>,
    pub upload_by: Option<String>,
    pub log: Vec<Value>,
}

impl Uploader {
    pub fn new(sites: Arc<Sites>, attempts: PathBuf) -> Self {
        Self {
            sites,
            attempts,
            changed: Box::new(|_| {}),
            now: Arc::new(crate::now_ms),
        }
    }

    /// Called with the local attempt id after every sidecar change.
    pub fn on_change(mut self, f: impl Fn(&str) + Send + Sync + 'static) -> Self {
        self.changed = Box::new(f);
        self
    }

    pub fn clock(mut self, now: impl Fn() -> i64 + Send + Sync + 'static) -> Self {
        self.now = Arc::new(now);
        self
    }

    /// The folder of a local attempt; ids are UUIDs, so nothing else can
    /// name a path outside the attempts folder.
    fn dir(&self, id: &str) -> Option<PathBuf> {
        let ok = id.len() == 36 && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-');
        let dir = self.attempts.join(id);
        (ok && dir.is_dir()).then_some(dir)
    }

    fn ctx<'a>(site: Option<&'a SiteRecord>, car: &'a Sidecar) -> Ctx<'a> {
        Self::ctx_of(site, &car.origin, car.upload_by.as_deref())
    }

    fn ctx_of<'a>(
        site: Option<&'a SiteRecord>,
        origin: &'a str,
        upload_by: Option<&'a str>,
    ) -> Ctx<'a> {
        Ctx {
            site: site.map_or(origin, |s| s.origin.host()),
            upload_by,
            max_bytes: site.map_or(crate::discovery::DEFAULT_MAX_UPLOAD, |s| {
                s.well_known.max_upload()
            }),
        }
    }

    fn save(
        &self,
        id: &str,
        dir: &Path,
        car: &mut Sidecar,
        to: SiteState,
        event: &str,
        fields: Value,
    ) {
        if let Err(e) = car.go(dir, to, event, fields) {
            eprintln!("attempt {id}: {e}");
        }
        (self.changed)(id);
    }

    /// Before placing (B5): when library entry `key` came from a site,
    /// asks it for an attempt under the local id and returns the marker
    /// `ref` it issued; offline or signed out, the attempt is practice. A
    /// local file gets no sidecar.
    pub fn issue(&self, id: &str, key: &str, definition_sha256: &str) -> Option<String> {
        let (site, linked) = self.sites.source(key)?;
        let dir = self.dir(id)?;
        let origin = site.origin.as_str();
        let user = site.user.as_ref().map(|u| u.id.clone());
        let mut car = Sidecar::new(origin, user, &linked.id, linked.version);
        let _ = car.write(&dir);
        let host = site.origin.host();
        let practice = |why: &str| format!("Practice: {why} Graded here, never uploaded.");
        let Some(session) = self.sites.session(origin).filter(|s| s.has_grant()) else {
            car.text = practice(&format!("signed out of {host} at the start."));
            self.save(
                id,
                &dir,
                &mut car,
                SiteState::Practice,
                "practice",
                json!({ "reason": "signed_out" }),
            );
            return None;
        };
        match session.issue(&linked.id, linked.version, id) {
            Ok((a, definition)) => {
                let now = (self.now)();
                let got = format!("{:x}", Sha256::digest(definition.get().as_bytes()));
                let matches = got == definition_sha256 && a.definition_sha256 == definition_sha256;
                car.attempt_id = Some(a.id.clone());
                car.marker_ref = Some(a.marker_ref.clone());
                car.definition_sha256 = Some(a.definition_sha256.clone());
                car.definition_match = Some(matches);
                car.fly_by_ms = Some(now + a.fly_within_s as i64 * 1000);
                car.upload_by_ms = Some(now + a.upload_within_s as i64 * 1000);
                car.upload_by = a.upload_by.clone();
                let fields = json!({
                    "origin": origin, "attempt_id": a.id, "ref": a.marker_ref,
                    "definition_sha256": a.definition_sha256, "definition_match": matches,
                    "fly_within_s": a.fly_within_s, "upload_within_s": a.upload_within_s,
                });
                if matches {
                    car.text = format!("Ranked on {host}: fly within {} min.", a.fly_within_s / 60);
                    self.save(id, &dir, &mut car, SiteState::Issued, "issued", fields);
                    Some(a.marker_ref)
                } else {
                    // The site would score another definition than the one flown.
                    let _ = session.abandon(&a.id);
                    car.text = practice(&format!(
                        "this challenge changed on {host}; take it again from Sites."
                    ));
                    self.save(id, &dir, &mut car, SiteState::Practice, "practice", fields);
                    None
                }
            }
            Err(e) => {
                car.text = practice(&copy::error(&e, Self::ctx(Some(&site), &car)));
                car.detail = detail(&e);
                let fields = json!({ "reason": e.code() });
                self.save(id, &dir, &mut car, SiteState::Practice, "practice", fields);
                None
            }
        }
    }

    /// The core finished (`done` or `sliced`): an issued attempt is queued.
    pub fn core_done(&self, id: &str) {
        self.after_core(
            id,
            SiteState::UploadPending,
            "Waiting to upload…",
            "upload_queued",
        );
    }

    /// The core abandoned the attempt: the site is told (queued if offline).
    pub fn core_abandoned(&self, id: &str) {
        self.after_core(
            id,
            SiteState::Abandoning,
            "Abandoning on the site…",
            "abandon_queued",
        );
    }

    fn after_core(&self, id: &str, to: SiteState, text: &str, event: &str) {
        let Some(dir) = self.dir(id) else { return };
        let Some(mut car) = Sidecar::read(&dir) else {
            return;
        };
        if car.state == SiteState::Issued {
            car.text = text.into();
            car.next_at_ms = Some((self.now)());
            self.save(id, &dir, &mut car, to, event, json!({}));
        }
    }

    /// Every local attempt with a sidecar.
    fn all(&self) -> Vec<(String, PathBuf, Sidecar)> {
        let entries = fs::read_dir(&self.attempts).into_iter().flatten().flatten();
        entries
            .filter_map(|e| {
                let id = e.file_name().to_str()?.to_owned();
                let car = Sidecar::read(&e.path())?;
                Some((id, e.path(), car))
            })
            .collect()
    }

    /// Makes queued and polled attempts due now: at start-up, on sign-in,
    /// when the Challenges screen opens. `origin` limits it to one site.
    pub fn wake(&self, origin: Option<&str>, id: Option<&str>) {
        let now = (self.now)();
        for (i, dir, mut car) in self.all() {
            let wanted = origin.is_none_or(|o| o == car.origin) && id.is_none_or(|x| x == i);
            let active = car.state.unsent() && car.state != SiteState::Issued
                || car.state.polled()
                || car.state == SiteState::Abandoning;
            if wanted && active {
                car.next_at_ms = Some(now);
                let _ = car.write(&dir);
            }
        }
    }

    /// Does what is due, one attempt at a time; returns when the next
    /// thing is due, if anything is.
    pub fn run_due(&self) -> Option<i64> {
        let mut next = None;
        for (id, dir, car) in self.all() {
            let Some(at) = car.next_at_ms else { continue };
            if at > (self.now)() {
                next = Some(next.map_or(at, |n: i64| n.min(at)));
                continue;
            }
            match car.state {
                SiteState::UploadPending | SiteState::UploadRetry => self.upload(&id, &dir, car),
                SiteState::Abandoning => self.abandon(&id, &dir, car),
                s if s.polled() => self.poll(&id, &dir, car),
                _ => {}
            }
            if let Some(at) = Sidecar::read(&dir).and_then(|c| c.next_at_ms) {
                next = Some(next.map_or(at, |n: i64| n.min(at)));
            }
        }
        next
    }

    /// Attempts on `origin` not yet sent: what "Sign out" warns about.
    pub fn pending(&self, origin: &str) -> usize {
        self.all()
            .iter()
            .filter(|(_, _, c)| c.origin == origin && c.state.unsent())
            .count()
    }

    fn upload(&self, id: &str, dir: &Path, mut car: Sidecar) {
        let now = (self.now)();
        let site = self.sites.get(&car.origin);
        let ctx_site = site.clone();
        let wait = |car: &mut Sidecar, text: String, next: Option<i64>| {
            car.text = text;
            car.next_at_ms = next;
            let _ = car.write(dir);
            (self.changed)(id);
        };
        let Some(site) = site else {
            return wait(
                &mut car,
                "The site was removed. Add it again to upload.".into(),
                None,
            );
        };
        let session = self.sites.session(&car.origin).filter(|s| s.has_grant());
        let Some(session) = session else {
            let text = copy::error(&Error::SignedOut, Self::ctx(Some(&site), &car));
            return wait(&mut car, text, None);
        };
        if let (Some(mine), Some(now_user)) = (&car.user_id, &site.user)
            && *mine != now_user.id
        {
            car.text = format!(
                "Issued to another account on {}; not uploaded.",
                site.origin.host()
            );
            car.next_at_ms = None;
            return self.save(
                id,
                dir,
                &mut car,
                SiteState::UploadAbandoned,
                "upload_abandoned",
                json!({}),
            );
        }
        if car.upload_by_ms.is_some_and(|by| now > by) {
            car.text = "Not uploaded in time.".into();
            car.next_at_ms = None;
            return self.save(id, dir, &mut car, SiteState::Expired, "expired", json!({}));
        }
        let path = dir.join(SLICE);
        if !path.is_file() {
            car.text = "This attempt has no recording slice, so it can't be uploaded.".into();
            car.next_at_ms = None;
            return self.save(
                id,
                dir,
                &mut car,
                SiteState::UploadRefused,
                "upload_refused",
                json!({ "reason": "no_slice" }),
            );
        }
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                return wait(
                    &mut car,
                    format!("Couldn't read the recording: {e}"),
                    Some(now + NO_RECORDING_S * 1000),
                );
            }
        };
        let (origin, upload_by) = (car.origin.clone(), car.upload_by.clone());
        let ctx = Self::ctx_of(ctx_site.as_ref(), &origin, upload_by.as_deref());
        if !bytes.starts_with(MAGIC) {
            car.text = "This recording isn't a compact flight, so it can't be uploaded.".into();
            return self.save(
                id,
                dir,
                &mut car,
                SiteState::UploadRefused,
                "upload_refused",
                json!({ "reason": "not_compact" }),
            );
        }
        if bytes.len() as u64 > ctx.max_bytes {
            car.text = format!(
                "This recording is larger than {} accepts ({} KB).",
                ctx.site,
                ctx.max_bytes / 1024
            );
            return self.save(
                id,
                dir,
                &mut car,
                SiteState::UploadRefused,
                "upload_refused",
                json!({ "reason": "too_large", "bytes": bytes.len() }),
            );
        }
        let sha = format!("{:x}", Sha256::digest(&bytes));
        let attempt = car.attempt_id.clone().unwrap_or_default();
        car.tries += 1;
        let started = std::time::Instant::now();
        let sent = session.upload(&attempt, &bytes, &sha);
        let ms = started.elapsed().as_millis() as u64;
        match sent {
            Ok(status) => {
                car.sha256 = Some(sha);
                car.uploaded_at_ms = Some(now);
                car.text = copy::status(&status, ctx.site);
                car.detail = None;
                car.next_at_ms = (!status.is_final()).then_some(now + 2000);
                let to = SiteState::from_server(&status.status);
                let fields = json!({ "try": car.tries, "http": 202, "status": status.status, "bytes": bytes.len(), "ms": ms });
                car.last = Some(status);
                self.save(id, dir, &mut car, to, "upload", fields);
                if let Some(result) = fs::read(dir.join(RESULT))
                    .ok()
                    .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                    && let Err(e) = session.client_result(&attempt, &result)
                {
                    sidecar::log(dir, "client_result", json!({ "error": e.code() }));
                }
            }
            Err(e) => {
                let mut fields = json!({ "try": car.tries, "error": e.code(), "ms": ms });
                if let Error::Api(api) = &e {
                    fields["http"] = api.http.into();
                    fields["request_id"] = api.request_id.clone().into();
                }
                car.text = copy::error(&e, ctx);
                car.detail = detail(&e);
                let mut retry_at = |s: i64| {
                    fields["retry_in_s"] = s.into();
                    Some(now + s * 1000)
                };
                let to = match copy::retry(&e) {
                    Retry::Checksum if car.checksum_tries < 3 => {
                        car.checksum_tries += 1;
                        car.next_at_ms = retry_at(0);
                        SiteState::UploadRetry
                    }
                    Retry::SignIn => {
                        car.next_at_ms = None;
                        car.state
                    }
                    Retry::After(Some(s)) => {
                        car.next_at_ms = retry_at(s as i64);
                        SiteState::UploadRetry
                    }
                    Retry::After(None) | Retry::Backoff => {
                        car.next_at_ms = retry_at(backoff(car.tries));
                        SiteState::UploadRetry
                    }
                    Retry::Checksum | Retry::No => {
                        car.next_at_ms = None;
                        match &e {
                            Error::Api(api) if api.http == 410 => SiteState::Expired,
                            _ => SiteState::UploadRefused,
                        }
                    }
                };
                self.save(id, dir, &mut car, to, "upload", fields);
            }
        }
    }

    fn poll(&self, id: &str, dir: &Path, mut car: Sidecar) {
        let now = (self.now)();
        let elapsed = now - car.uploaded_at_ms.unwrap_or(now);
        // Every 2 s for a minute, every 30 s for 10 min, then on wake only.
        let next = if elapsed < 60_000 {
            Some(now + 2000)
        } else if elapsed < 660_000 {
            Some(now + 30_000)
        } else {
            None
        };
        let site = self.sites.get(&car.origin);
        car.next_at_ms = next;
        let Some(session) = self.sites.session(&car.origin) else {
            // The site was removed: nothing to ask until it is added again.
            car.next_at_ms = None;
            let _ = car.write(dir);
            return;
        };
        let attempt = car.attempt_id.clone().unwrap_or_default();
        match session.attempt(&attempt) {
            Ok(status) if Some(&status) != car.last.as_ref() => {
                let ctx = Self::ctx(site.as_ref(), &car);
                car.text = copy::status(&status, ctx.site);
                if status.is_final() {
                    car.next_at_ms = None;
                }
                let to = SiteState::from_server(&status.status);
                let fields = json!({ "status": status.status, "score": status.score(), "verified": status.verified, "rank": status.rank });
                car.last = Some(status);
                self.save(id, dir, &mut car, to, "server_status", fields);
            }
            Ok(_) => {
                let _ = car.write(dir);
            }
            Err(e) => {
                if copy::retry(&e) == Retry::SignIn {
                    car.text = copy::error(&e, Self::ctx(site.as_ref(), &car));
                }
                let _ = car.write(dir);
                (self.changed)(id);
            }
        }
    }

    fn abandon(&self, id: &str, dir: &Path, mut car: Sidecar) {
        let now = (self.now)();
        let Some(session) = self.sites.session(&car.origin).filter(|s| s.has_grant()) else {
            car.next_at_ms = None;
            let _ = car.write(dir);
            return;
        };
        let attempt = car.attempt_id.clone().unwrap_or_default();
        car.tries += 1;
        match session.abandon(&attempt) {
            Err(e) if matches!(copy::retry(&e), Retry::Backoff | Retry::After(_)) => {
                car.next_at_ms = Some(now + backoff(car.tries) * 1000);
                let _ = car.write(dir);
            }
            outcome => {
                car.text = "Abandoned.".into();
                car.next_at_ms = None;
                let fields = json!({ "outcome": outcome.as_ref().map_or_else(|e| e.code().to_owned(), |_| "ok".into()) });
                self.save(id, dir, &mut car, SiteState::Abandoned, "abandoned", fields);
            }
        }
    }

    /// Every attempt with a sidecar, for the Challenges screen.
    pub fn views(&self) -> Vec<View> {
        self.all()
            .into_iter()
            .map(|(id, dir, car)| {
                let site = self.sites.get(&car.origin);
                let template = site
                    .as_ref()
                    .and_then(|s| s.well_known.links.attempt.clone());
                let url = car
                    .last
                    .as_ref()
                    .and_then(|s| s.url.clone())
                    .filter(|u| site.as_ref().is_some_and(|s| s.origin.owns(u)))
                    .or_else(|| {
                        let attempt = car.attempt_id.as_deref()?;
                        let enc = |s: &str| {
                            url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>()
                        };
                        Some(
                            template?
                                .replace("{id}", &enc(&car.challenge))
                                .replace("{attempt}", &enc(attempt)),
                        )
                    });
                View {
                    site: site.as_ref().map_or(car.origin.clone(), SiteRecord::label),
                    origin: car.origin.clone(),
                    state: car.state,
                    text: car.text.clone(),
                    detail: car.detail.clone(),
                    status: car.last.clone(),
                    upload_by: car.upload_by.clone(),
                    log: sidecar::read_log(&dir),
                    url,
                    id,
                }
            })
            .collect()
    }
}

fn backoff(tries: u32) -> i64 {
    BACKOFF_S
        .get(tries.saturating_sub(1) as usize)
        .copied()
        .unwrap_or(HOUR_S)
}

fn detail(e: &Error) -> Option<String> {
    match e {
        Error::Api(api) if !api.message.is_empty() => Some(api.message.clone()),
        Error::Network(m) | Error::Protocol(m) => Some(m.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock;
    use crate::mock::{Mock, Reply, tokens, well_known};
    use crate::sites::tests::sites;
    use crate::store::{MemoryStore, SecretStore};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicI64, Ordering};

    const ID: &str = "6f1c2d3e-4a5b-4c6d-8e7f-9a0b1c2d3e4f";
    const DEFINITION: &str = r#"{"id":"ltfj-06","version":1}"#;

    fn sha(s: &[u8]) -> String {
        format!("{:x}", Sha256::digest(s))
    }

    struct World {
        mock: Mock,
        _tmp: tempfile::TempDir,
        sites: Arc<Sites>,
        store: Arc<MemoryStore>,
        up: Uploader,
        clock: Arc<AtomicI64>,
        origin: String,
        dir: PathBuf,
    }

    /// A site that answers uploads with `uploads` in order, and one local
    /// attempt at its challenge, flown with a flight file on disk.
    fn world(uploads: Vec<Reply>) -> World {
        let uploads = Mutex::new(uploads);
        let mock = Mock::start(move |req, base| {
            match (req.method.as_str(), req.path.as_str()) {
            (_, crate::discovery::PATH) => Reply::json(200, well_known(base)),
            ("POST", "/api/foqa/v1/challenges/ltfj-06/attempts") => Reply::raw(201, "application/json", format!(
                r#"{{"attempt":{{"id":"srv-1","ref":"Qe3v1hJ0dEa9fP2kLm7xRw","challenge_id":"ltfj-06","challenge_version":1,"definition_sha256":"{}","issued_at":"t","fly_within_s":1800,"upload_within_s":86400,"upload_by":"28 Oct 18:02"}},"definition":{DEFINITION}}}"#,
                sha(DEFINITION.as_bytes())
            ).into_bytes()),
            ("PUT", "/api/foqa/v1/attempts/srv-1/recording") => {
                let mut u = lock(&uploads);
                if u.is_empty() { Reply::json(202, json!({ "attempt": { "id": "srv-1", "status": "queued" } })) } else { u.remove(0) }
            }
            ("PUT", "/api/foqa/v1/attempts/srv-1/client-result") => Reply::raw(204, "application/json", vec![]),
            ("GET", "/api/foqa/v1/attempts/srv-1") => Reply::json(200, json!({ "attempt": {
                "id": "srv-1", "status": "scored", "verified": true, "rank": 3,
                "result": { "score": { "total": 87.4 } }, "url": format!("{base}/#/a/srv-1")
            }})),
            ("POST", "/api/foqa/v1/device/code") => Reply::json(200, json!({
                "device_code": "dc", "user_code": "X", "verification_uri": format!("{base}/#/device"), "expires_in": 60, "interval": 0
            })),
            ("POST", "/api/foqa/v1/token") => Reply::json(200, tokens(1)),
            ("POST", "/api/foqa/v1/attempts/srv-1/abandon") => Reply::json(200, json!({ "attempt": { "id": "srv-1", "status": "abandoned" } })),
            _ => Reply::error(404, "not_found"),
        }
        });
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(MemoryStore::default());
        let sites = sites(tmp.path(), store.clone());
        let origin = sites.add(&mock.base).unwrap().origin.as_str().to_owned();
        sites.link(&origin, "ltfj-06-v1", "ltfj-06", 1).unwrap();
        sites
            .sign_in(&origin, crate::sites::Flow::Device(&|_, _| {}))
            .unwrap();
        let attempts = tmp.path().join("attempts");
        let dir = attempts.join(ID);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(SLICE), b"FQJC slice").unwrap();
        let clock = Arc::new(AtomicI64::new(1_000_000));
        let c = Arc::clone(&clock);
        let up =
            Uploader::new(Arc::clone(&sites), attempts).clock(move || c.load(Ordering::SeqCst));
        World {
            mock,
            _tmp: tmp,
            sites,
            store,
            up,
            clock,
            origin,
            dir,
        }
    }

    impl World {
        fn car(&self) -> Sidecar {
            Sidecar::read(&self.dir).unwrap()
        }
        fn advance(&self, ms: i64) {
            self.clock.fetch_add(ms, Ordering::SeqCst);
        }
        fn issue(&self) -> Option<String> {
            self.up.issue(ID, "ltfj-06-v1", &sha(DEFINITION.as_bytes()))
        }
    }

    #[test]
    fn an_issued_attempt_uploads_its_recording_and_polls_to_its_score() {
        let w = world(vec![]);
        assert_eq!(w.up.issue(ID, "a-local-file-v1", "x"), None);
        assert!(
            Sidecar::read(&w.dir).is_none(),
            "a local file gets no sidecar"
        );
        assert_eq!(w.issue().as_deref(), Some("Qe3v1hJ0dEa9fP2kLm7xRw"));
        let car = w.car();
        assert_eq!(
            (car.state, car.attempt_id.as_deref(), car.definition_match),
            (SiteState::Issued, Some("srv-1"), Some(true))
        );
        assert_eq!(
            car.upload_by_ms,
            Some(1_000_000 + 86_400_000),
            "deadline from the response's arrival"
        );
        let issue = w
            .mock
            .requests()
            .into_iter()
            .find(|r| r.path.ends_with("/attempts"))
            .unwrap();
        assert_eq!(issue.json()["client_attempt_id"], ID);
        assert_eq!(w.up.pending(&w.origin), 1);

        w.up.core_done(ID);
        assert_eq!(w.car().state, SiteState::UploadPending);
        w.up.run_due();
        let car = w.car();
        assert_eq!(
            (car.state, car.text.as_str()),
            (SiteState::Queued, "Uploaded. Scoring…")
        );
        let put = w
            .mock
            .requests()
            .into_iter()
            .find(|r| r.path.ends_with("/recording"))
            .unwrap();
        assert_eq!(put.body, b"FQJC slice");
        assert_eq!(
            put.header("x-foqa-sha256"),
            Some(sha(b"FQJC slice").as_str())
        );
        assert_eq!(put.header("content-type"), Some("application/octet-stream"));
        assert_eq!(w.up.pending(&w.origin), 0);

        assert_eq!(w.up.run_due(), Some(1_002_000), "polled 2 s later");
        w.advance(2000);
        w.up.run_due();
        let car = w.car();
        assert_eq!(
            (car.state, car.text.as_str(), car.next_at_ms),
            (SiteState::Scored, "Scored 87.4, #3", None)
        );
        let view = &w.up.views()[0];
        assert!(view.url.as_deref().unwrap().ends_with("/#/a/srv-1"));
        let events: Vec<_> = view
            .log
            .iter()
            .map(|l| l["event"].as_str().unwrap())
            .collect();
        assert_eq!(
            events,
            ["issued", "upload_queued", "upload", "server_status"]
        );
    }

    #[test]
    fn without_a_slice_nothing_is_uploaded() {
        let w = world(vec![]);
        w.issue();
        fs::remove_file(w.dir.join(SLICE)).unwrap();
        w.up.core_done(ID);
        w.up.run_due();
        let car = w.car();
        assert_eq!(
            (car.state, car.next_at_ms),
            (SiteState::UploadRefused, None)
        );
        assert!(car.text.contains("no recording slice"), "{}", car.text);
        assert_eq!(w.mock.hits("/recording"), 0, "never the whole flight");
    }

    #[test]
    fn failures_back_off_retry_after_wins_and_the_same_bytes_are_resent() {
        let w = world(vec![
            Reply::error(429, "busy").header("Retry-After", "30"),
            Reply::error(500, "oops"),
            Reply::error(400, "checksum_mismatch"),
            Reply::error(400, "checksum_mismatch"),
            Reply::error(400, "checksum_mismatch"),
            Reply::error(400, "checksum_mismatch"),
        ]);
        w.issue();
        w.up.core_done(ID);
        w.up.run_due();
        let car = w.car();
        assert_eq!(
            (car.state, car.next_at_ms),
            (SiteState::UploadRetry, Some(1_030_000)),
            "Retry-After wins"
        );
        assert!(car.text.contains("busy. Retrying in 30 s"), "{}", car.text);
        assert_eq!(w.up.run_due(), Some(1_030_000), "not due yet");
        w.advance(30_000);
        w.up.run_due();
        assert_eq!(
            w.car().next_at_ms,
            Some(1_030_000 + 30_000),
            "second try: 30 s backoff"
        );
        w.advance(30_000);
        for _ in 0..4 {
            w.up.run_due();
        }
        let car = w.car();
        assert_eq!(
            (car.state, car.checksum_tries),
            (SiteState::UploadRefused, 3),
            "checksum retried 3 times"
        );
        let shas: Vec<_> = w
            .mock
            .requests()
            .iter()
            .filter(|r| r.path.ends_with("/recording"))
            .map(|r| r.header("x-foqa-sha256").unwrap().to_owned())
            .collect();
        assert_eq!(shas.len(), 6);
        assert!(
            shas.iter().all(|s| *s == shas[0]),
            "a retry sends the same bytes"
        );
    }

    #[test]
    fn offline_it_stays_queued_and_a_lost_window_expires() {
        let w = world(vec![]);
        w.issue();
        w.up.core_done(ID);
        // The site goes away: network errors back off, still queued.
        drop(w.mock);
        w.up.run_due();
        let car = Sidecar::read(&w.dir).unwrap();
        assert_eq!(car.state, SiteState::UploadRetry);
        assert!(car.text.starts_with("Can't reach"), "{}", car.text);
        assert_eq!(car.next_at_ms, Some(1_005_000), "first backoff: 5 s");
        assert_eq!(w.up.pending(&w.origin), 1);
        // Past the local deadline, it is not tried again.
        w.clock.fetch_add(86_400_001, Ordering::SeqCst);
        w.up.run_due();
        assert_eq!(Sidecar::read(&w.dir).unwrap().state, SiteState::Expired);
    }

    #[test]
    fn signed_out_uploads_wait_and_another_account_never_sends_them() {
        let w = world(vec![]);
        w.issue();
        w.up.core_done(ID);
        w.sites.session(&w.origin).unwrap().drop_grant();
        w.up.run_due();
        let car = w.car();
        assert_eq!(
            (car.state, car.next_at_ms),
            (SiteState::UploadPending, None)
        );
        assert!(car.text.starts_with("Signed out of"), "{}", car.text);
        // Back as another account.
        w.store.set(&w.origin, "refresh-1").unwrap();
        let mut car = w.car();
        car.user_id = Some("someone-else".into());
        car.write(&w.dir).unwrap();
        w.up.wake(Some(&w.origin), None);
        w.up.run_due();
        assert_eq!(w.car().state, SiteState::UploadAbandoned);
        assert_eq!(w.mock.hits("/recording"), 0);
    }

    #[test]
    fn practice_when_signed_out_at_the_start_and_abandon_tells_the_site() {
        let w = world(vec![]);
        w.sites.session(&w.origin).unwrap().drop_grant();
        assert_eq!(w.issue(), None, "practice has no marker ref");
        let car = w.car();
        assert_eq!(car.state, SiteState::Practice);
        assert!(
            car.text.starts_with("Practice: signed out of"),
            "{}",
            car.text
        );
        w.up.core_done(ID);
        assert_eq!(
            w.car().state,
            SiteState::Practice,
            "practice is never uploaded"
        );

        let w = world(vec![]);
        w.issue();
        w.up.core_abandoned(ID);
        w.up.run_due();
        assert_eq!(w.car().state, SiteState::Abandoned);
        assert_eq!(w.mock.hits("/abandon"), 1);
    }

    #[test]
    fn a_definition_other_than_the_one_issued_is_practice() {
        let w = world(vec![]);
        assert_eq!(
            w.up.issue(ID, "ltfj-06-v1", &sha(b"another definition")),
            None
        );
        let car = w.car();
        assert_eq!(
            (car.state, car.definition_match),
            (SiteState::Practice, Some(false))
        );
        assert_eq!(
            w.mock.hits("/abandon"),
            1,
            "the issued attempt is given back"
        );
    }

    #[test]
    fn backoff_steps_then_hourly() {
        let steps: Vec<_> = (1..=6).map(backoff).collect();
        assert_eq!(steps, [5, 30, 120, 600, 3600, 3600]);
    }
}
