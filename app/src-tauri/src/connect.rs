//! The `connect` feature (docs/challenge-protocol.md B1): the site client
//! joined to the app. Sites settings, challenges taken from a site, issue
//! before placing, the background uploader, deep links from websites (B2),
//! toasts on a site's verdict (B7) and its Tauri commands. The app builds
//! and runs without this module.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use ::connect::api::Error;
use ::connect::copy::{self, Ctx};
use ::connect::deeplink::{self, Consent, Gate, Link, Pending};
use ::connect::http::Http;
use ::connect::sidecar::SiteState;
use ::connect::sites::{Flow, SiteRecord, Sites};
use ::connect::store::KeyringStore;
use ::connect::uploader::{self, Uploader};
use capture::SiteStrip;
use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_notification::NotificationExt;

use crate::attempts::{Record, State as AttemptState};
use crate::commands::{AttemptHooks, Shared, lock};
use crate::library::{Entry, sha256_hex};

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
/// The uploader looks at its queue at least this often.
const IDLE: Duration = Duration::from_secs(60);

pub struct Connect {
    sites: Arc<Sites>,
    uploader: Uploader,
    wake: Mutex<Sender<()>>,
    /// The device code shown while a device sign-in waits, per origin.
    device: Mutex<HashMap<String, Device>>,
    app: AppHandle,
    /// "Allow local sites" (A1).
    allow_local: bool,
    /// The deep link waiting for the user, and the last one refused.
    links: Mutex<Gate>,
    refused: Mutex<Option<&'static str>>,
    /// The attempt folders, for a toast's challenge title.
    attempts: PathBuf,
    /// Attempts whose verdict was shown in a toast.
    toasted: Mutex<HashSet<String>>,
}

/// A deep link as the app asks about it (`deep-link` event).
#[derive(Debug, Clone, Serialize)]
pub struct LinkView {
    /// `ask`: "Connect to <host>?"; `open`: the site is the user's, open
    /// the challenge; `refused`: the link is invalid (`text` says why).
    state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    origin: Option<String>,
    /// The host in ASCII (punycode for an IDN host), shown before any name.
    #[serde(skip_serializing_if = "Option::is_none")]
    host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    v: Option<u32>,
    signed_in: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Device {
    user_code: String,
    verification_uri: String,
}

/// A site as the Sites list shows it.
#[derive(Debug, Clone, Serialize)]
pub struct SiteView {
    origin: String,
    /// The host in ASCII (punycode for an IDN host).
    host: String,
    /// Self-declared; shown next to the host, never instead of it.
    name: String,
    handle: Option<String>,
    signed_in: bool,
    /// Attempts on this site not uploaded yet.
    pending: usize,
    device: Option<Device>,
    account_url: Option<String>,
}

/// A challenge a site publishes.
#[derive(Debug, Clone, Serialize)]
pub struct SiteChallenge {
    id: String,
    version: u32,
    title: String,
    state: String,
    opens_at: Option<String>,
    closes_at: Option<String>,
    /// The library key once taken.
    key: Option<String>,
}

impl Connect {
    /// `data` is the app's data folder; `attempts` the attempt folders.
    pub fn start(data: PathBuf, attempts: PathBuf, app: AppHandle) -> std::io::Result<Arc<Self>> {
        // Developer setting "Allow local sites" (A1); off by default.
        let allow_local = std::env::var_os("FOQA_ALLOW_LOCAL_SITES").is_some();
        let http = Arc::new(Http::new(APP_VERSION));
        let sites = Arc::new(Sites::open(
            data.join("connect"),
            http,
            Arc::new(KeyringStore),
            allow_local,
        )?);
        let handle = app.clone();
        let (changed, verdicts) = mpsc::channel::<String>();
        let changed = Mutex::new(changed);
        let uploader = Uploader::new(Arc::clone(&sites), attempts.clone()).on_change(move |id| {
            emit(&handle, json!({ "attempt": id }));
            let _ = lock(&changed).send(id.to_owned());
        });
        let (tx, rx) = mpsc::channel();
        let connect = Arc::new(Self {
            sites,
            uploader,
            wake: Mutex::new(tx),
            device: Mutex::new(HashMap::new()),
            app,
            allow_local,
            links: Mutex::new(Gate::default()),
            refused: Mutex::new(None),
            attempts,
            toasted: Mutex::new(HashSet::new()),
        });
        let toasts = Arc::clone(&connect);
        thread::spawn(move || verdicts.iter().for_each(|id| toasts.toast(&id)));
        connect.uploader.wake(None, None);
        let run = Arc::clone(&connect);
        thread::Builder::new()
            .name("uploader".into())
            .spawn(move || run.run(rx))?;
        Ok(connect)
    }

    /// One attempt at a time, until the app quits.
    fn run(&self, wake: Receiver<()>) {
        loop {
            let next = self.uploader.run_due();
            let wait = next.map_or(IDLE, |at| {
                let ms = (at - ::connect::now_ms()).clamp(0, IDLE.as_millis() as i64);
                Duration::from_millis(ms as u64)
            });
            if let Err(RecvTimeoutError::Disconnected) = wake.recv_timeout(wait) {
                return;
            }
        }
    }

    fn kick(&self) {
        let _ = lock(&self.wake).send(());
    }

    /// The core changed an attempt: sliced → upload (`done` too: one done
    /// without a slice is refused), abandoned or done with a slice that
    /// can never be cut → tell the site, with why.
    pub fn attempt_changed(&self, record: &Record) {
        let id = &record.id;
        match (record.state, &record.slice_failed) {
            (AttemptState::Done, Some(why)) => self.uploader.core_abandoned(id, why),
            (AttemptState::Sliced | AttemptState::Done, _) => self.uploader.core_done(id),
            (AttemptState::Abandoned | AttemptState::FailedStart, _) => {
                let why = record.reason.as_deref().unwrap_or("failed_start");
                self.uploader.core_abandoned(id, why)
            }
            _ => return,
        }
        self.kick();
    }

    fn view(&self, r: &SiteRecord) -> SiteView {
        let origin = r.origin.as_str();
        SiteView {
            origin: origin.to_owned(),
            host: r.origin.host().to_owned(),
            name: r.well_known.name.clone(),
            handle: r.user.as_ref().and_then(|u| u.handle.clone()),
            signed_in: self.sites.signed_in(origin),
            pending: self.uploader.pending(origin),
            device: lock(&self.device).get(origin).cloned(),
            account_url: r.well_known.links.account.clone(),
        }
    }

    fn site(&self, origin: &str) -> Result<SiteRecord, String> {
        self.sites
            .get(origin)
            .ok_or_else(|| format!("{origin} isn't one of your sites."))
    }

    /// A link from a website (B2), forwarded or at a cold start: parsed
    /// strictly and logged; it only brings the window up and asks. Nothing
    /// is fetched from a site the user hasn't connected.
    pub fn open_url(&self, raw: &str) {
        match deeplink::parse(raw, self.allow_local) {
            Err(refused) => {
                self.sites.log(json!({
                    "event": "deep_link", "accepted": false, "reason": refused.0, "bytes": raw.len(),
                }));
                *lock(&self.refused) = Some(refused.text());
            }
            Ok(link) => {
                let (id, v) = match &link {
                    Link::Challenge { id, version, .. } => (Some(id.as_str()), *version),
                    Link::Connect { .. } => (None, None),
                };
                self.sites.log(json!({
                    "event": "deep_link", "accepted": true, "kind": link.kind(),
                    "origin": link.site(), "id": id, "v": v,
                }));
                let known = self.sites.get(link.site().as_str()).is_some();
                lock(&self.links).offer(link, known, ::connect::now_ms());
            }
        }
        if let Some(view) = self.pending_link(false) {
            let _ = self.app.emit("deep-link", view);
        }
        crate::show(&self.app, None);
    }

    /// The link to ask about; a refused one is told once (`take`).
    fn pending_link(&self, take: bool) -> Option<LinkView> {
        let refused = if take {
            lock(&self.refused).take()
        } else {
            *lock(&self.refused)
        };
        if let Some(text) = refused {
            return Some(LinkView {
                state: "refused",
                kind: None,
                origin: None,
                host: None,
                name: None,
                id: None,
                v: None,
                signed_in: false,
                text: Some(text),
            });
        }
        let Pending { link, consent, .. } = lock(&self.links).pending(::connect::now_ms())?;
        let site = self.sites.get(link.site().as_str());
        let (id, v) = match &link {
            Link::Challenge { id, version, .. } => (Some(id.clone()), *version),
            Link::Connect { .. } => (None, None),
        };
        Some(LinkView {
            state: match consent {
                Consent::Ask => "ask",
                Consent::Open => "open",
            },
            kind: Some(link.kind()),
            origin: Some(link.site().as_str().to_owned()),
            host: Some(link.site().host().to_owned()),
            name: site.map(|s| s.well_known.name).filter(|n| !n.is_empty()),
            id,
            v,
            signed_in: self.sites.signed_in(link.site().as_str()),
            text: None,
        })
    }

    /// A toast when a site scores or rejects an attempt (B7), once, while
    /// the window isn't in front; a click opens the challenge.
    fn toast(&self, id: &str) {
        let Some(v) = self.uploader.views().into_iter().find(|v| v.id == id) else {
            return;
        };
        if !matches!(v.state, SiteState::Scored | SiteState::Rejected)
            || !lock(&self.toasted).insert(id.to_owned())
        {
            return;
        }
        let window = self.app.get_webview_window("main");
        let in_front = |w: tauri::WebviewWindow| {
            w.is_focused().unwrap_or(false) && w.is_visible().unwrap_or(false)
        };
        if window.is_some_and(in_front) {
            return;
        }
        let (title, key) = attempt_title(&self.attempts, id);
        let (head, body) = toast_text(v.state, &v.text, &v.site);
        let mut toast = self
            .app
            .notification()
            .builder()
            .title(format!("{head} on {title}"))
            .body(body);
        if let Some(key) = key {
            toast = toast.extra(crate::CHALLENGE_KEY, key);
        }
        if let Err(e) = toast.show() {
            eprintln!("notification not shown: {e}");
        }
    }
}

/// A verdict toast's headline and body from the attempt's site copy (A6):
/// "Scored 87.4, #3" / site; "Scored 81.0, not ranked" / why; "Rejected" / why.
fn toast_text(state: SiteState, text: &str, site: &str) -> (String, String) {
    match (state, text.split_once(": ")) {
        (SiteState::Rejected, _) => ("Rejected".into(), text.into()),
        (_, Some((head, why))) => (head.into(), why.into()),
        (_, None) => (text.trim_end_matches('.').into(), site.into()),
    }
}

/// The challenge title and library key of a local attempt.
fn attempt_title(attempts: &Path, id: &str) -> (String, Option<String>) {
    let record = std::fs::read(attempts.join(id).join("attempt.json")).ok();
    let v: serde_json::Value = record
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let title = v["title"].as_str().unwrap_or("the challenge").to_owned();
    (title, v["key"].as_str().map(str::to_owned))
}

impl AttemptHooks for Connect {
    fn before_place(&self, record: &Record) -> Option<String> {
        self.uploader
            .issue(&record.id, &record.key, &record.definition_sha256)
    }

    fn ranking(&self, key: &str) -> Option<(&'static str, String, Option<String>)> {
        let (site, _) = self.sites.source(key)?;
        let signed_in = self.sites.signed_in(site.origin.as_str());
        Some(copy::ranking(&site, signed_in))
    }

    fn source(&self, key: &str) -> Option<String> {
        self.sites.source(key).map(|(site, _)| site.label())
    }

    fn site_strip(&self, id: &str) -> Option<(SiteStrip, bool)> {
        use SiteState::*;
        let v = self.uploader.views().into_iter().find(|v| v.id == id)?;
        let status = v.status.as_ref();
        let state = match v.state {
            Issuing | Issued => "waiting",
            UploadPending | UploadRetry if !self.sites.signed_in(&v.origin) => "signed_out",
            UploadPending | UploadRetry => "uploading",
            Queued | Scoring => "scoring",
            Scored if status.is_some_and(|s| s.verified) => "scored",
            Scored => "unverified",
            Rejected => "rejected",
            Expired => "expired",
            Practice => "practice",
            _ => "unknown",
        };
        let strip = SiteStrip {
            state: state.into(),
            score: status.and_then(|s| s.score()),
            rank: status.and_then(|s| s.rank),
            reason: Some(v.text),
        };
        Some((strip, v.state.is_final()))
    }
}

fn emit(app: &AppHandle, payload: serde_json::Value) {
    if let Err(e) = app.emit("site-changed", payload) {
        eprintln!("event not sent: {e}");
    }
}

/// Runs a network call off the IPC thread.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}

type Connected<'a> = State<'a, Arc<Connect>>;

#[tauri::command]
pub fn connect_available() -> bool {
    true
}

/// The deep link waiting for the user, asked when the window is ready so
/// a link that started the app is not lost.
#[tauri::command]
pub fn deep_link_pending(c: Connected<'_>) -> Option<LinkView> {
    c.pending_link(true)
}

/// The answer to "Connect to <host>?" for the link waiting: allowed, the
/// site is added (its well-known document fetched only now) and the link
/// stays, to open the challenge after sign-in; denied, it is dropped.
#[tauri::command]
pub async fn deep_link_consent(
    origin: String,
    allow: bool,
    c: Connected<'_>,
) -> Result<Option<SiteView>, String> {
    let c = Arc::clone(&c);
    blocking(move || {
        let answered = lock(&c.links).consent(&origin, allow, ::connect::now_ms());
        let decision = if allow { "allow" } else { "deny" };
        c.sites.log(json!({
            "event": "consent", "origin": origin, "decision": decision, "waiting": answered.is_ok(),
        }));
        let Some(link) = answered? else {
            return Ok(None);
        };
        let added = c.sites.add(link.site().as_str());
        let r = added.inspect_err(|_| lock(&c.links).done())?;
        emit(&c.app, json!({ "origin": r.origin }));
        Ok(Some(c.view(&r)))
    })
    .await
}

/// The link was acted on or dismissed.
#[tauri::command]
pub fn deep_link_done(c: Connected<'_>) {
    lock(&c.links).done();
}

#[tauri::command]
pub fn sites_list(c: Connected<'_>) -> Vec<SiteView> {
    c.sites.list().iter().map(|r| c.view(r)).collect()
}

/// Adds a site by the address the user typed.
#[tauri::command]
pub async fn site_add(origin: String, c: Connected<'_>) -> Result<SiteView, String> {
    let c = Arc::clone(&c);
    blocking(move || {
        let r = c.sites.add(&origin)?;
        emit(&c.app, json!({ "origin": r.origin }));
        Ok(c.view(&r))
    })
    .await
}

/// Signs in through the browser, or with a code (`device`) when the
/// browser is elsewhere or the loopback can't be used.
#[tauri::command]
pub async fn site_sign_in(
    origin: String,
    device: bool,
    c: Connected<'_>,
) -> Result<SiteView, String> {
    let c = Arc::clone(&c);
    blocking(move || {
        let open = |url: &str| open::that_detached(url).map_err(|e| e.to_string());
        let show = |code: &::connect::auth::DeviceCode, page: Option<&str>| {
            let shown = Device {
                user_code: code.user_code.clone(),
                verification_uri: code.verification_uri.clone(),
            };
            lock(&c.device).insert(origin.clone(), shown);
            emit(&c.app, json!({ "origin": origin }));
            if let Some(page) = page {
                let _ = open::that_detached(page);
            }
        };
        let flow = if device {
            Flow::Device(&show)
        } else {
            Flow::Browser(&open)
        };
        let signed = c.sites.sign_in(&origin, flow);
        lock(&c.device).remove(&origin);
        emit(&c.app, json!({ "origin": origin }));
        signed?;
        c.uploader.wake(Some(&origin), None);
        c.kick();
        Ok(c.view(&c.site(&origin)?))
    })
    .await
}

#[tauri::command]
pub fn site_sign_in_cancel(origin: String, c: Connected<'_>) {
    c.sites.cancel_sign_in(&origin);
}

/// Revokes and deletes the credential. Pending uploads wait for the same
/// account to sign in again.
#[tauri::command]
pub async fn site_sign_out(origin: String, c: Connected<'_>) -> Result<SiteView, String> {
    let c = Arc::clone(&c);
    blocking(move || {
        c.sites.sign_out(&origin)?;
        emit(&c.app, json!({ "origin": origin }));
        Ok(c.view(&c.site(&origin)?))
    })
    .await
}

/// Signs out and forgets the site; its attempts stay.
#[tauri::command]
pub async fn site_remove(origin: String, c: Connected<'_>) -> Result<(), String> {
    let c = Arc::clone(&c);
    blocking(move || {
        c.sites.remove(&origin)?;
        emit(&c.app, json!({ "origin": origin }));
        Ok(())
    })
    .await
}

/// The site's open and upcoming challenges.
#[tauri::command]
pub async fn site_challenges(
    origin: String,
    c: Connected<'_>,
) -> Result<Vec<SiteChallenge>, String> {
    let c = Arc::clone(&c);
    blocking(move || {
        let site = c.site(&origin)?;
        c.sites.freshen(&origin);
        let session = c.sites.session(&origin).ok_or("no site")?;
        let entries = session.challenges().map_err(|e| plain(&e, &site))?;
        Ok(entries
            .into_iter()
            .map(|e| {
                let key = site
                    .challenges
                    .iter()
                    .find(|l| l.id == e.id && l.version == e.version)
                    .map(|l| l.key.clone());
                SiteChallenge {
                    id: e.id,
                    version: e.version,
                    title: e.title,
                    state: e.state,
                    opens_at: e.opens_at,
                    closes_at: e.closes_at,
                    key,
                }
            })
            .collect())
    })
    .await
}

/// Takes a site's challenge into the library: its exact bytes, checked
/// against the site's SHA-256 and by `Challenge::parse`.
#[tauri::command]
pub async fn site_challenge_take(
    origin: String,
    id: String,
    c: Connected<'_>,
    shared: State<'_, Arc<Shared>>,
) -> Result<Entry, String> {
    let (c, shared) = (Arc::clone(&c), Arc::clone(&shared));
    blocking(move || {
        let site = c.site(&origin)?;
        let session = c.sites.session(&origin).ok_or("no site")?;
        let entries = session.challenges().map_err(|e| plain(&e, &site))?;
        let entry = entries
            .into_iter()
            .find(|e| e.id == id)
            .ok_or_else(|| format!("This challenge is no longer on {}.", site.origin.host()))?;
        let bytes = entry.definition.get();
        if sha256_hex(bytes.as_bytes()) != entry.definition_sha256 {
            return Err(format!(
                "{} sent a challenge that doesn't match its checksum.",
                site.origin.host()
            ));
        }
        let kept = shared.import_challenge(bytes)?;
        c.sites.link(&origin, &kept.key, &entry.id, entry.version)?;
        emit(&c.app, json!({ "origin": origin }));
        Ok(kept)
    })
    .await
}

/// Every attempt's site part: state, plain copy and the server's result.
#[tauri::command]
pub fn site_attempts(c: Connected<'_>) -> Vec<uploader::View> {
    c.uploader.views()
}

/// Uploads (or polls) now: the Retry button, and the Challenges screen
/// opening (`id` none).
#[tauri::command]
pub fn attempt_upload(id: Option<String>, c: Connected<'_>) {
    c.uploader.wake(None, id.as_deref());
    c.kick();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_verdict_toast_reads_like_the_site_copy() {
        let t = |state, text: &str| toast_text(state, text, "Site (site.example)");
        assert_eq!(
            t(SiteState::Scored, "Scored 87.4, #3"),
            ("Scored 87.4, #3".into(), "Site (site.example)".into())
        );
        assert_eq!(
            t(
                SiteState::Scored,
                "Scored 81.0, not ranked: Flown outside the challenge's dates."
            ),
            (
                "Scored 81.0, not ranked".into(),
                "Flown outside the challenge's dates.".into()
            )
        );
        assert_eq!(
            t(SiteState::Scored, "Scored 81.0, not ranked."),
            (
                "Scored 81.0, not ranked".into(),
                "Site (site.example)".into()
            )
        );
        let rejected = t(SiteState::Rejected, "The recording couldn't be read.");
        assert_eq!(
            rejected,
            ("Rejected".into(), "The recording couldn't be read.".into())
        );
    }
}

fn plain(e: &Error, site: &SiteRecord) -> String {
    let ctx = Ctx {
        site: site.origin.host(),
        upload_by: None,
        max_bytes: site.well_known.max_upload(),
    };
    copy::error(e, ctx)
}

/// Opens a page of one of the user's sites in the browser; any other
/// address is refused.
#[tauri::command]
pub fn site_open(url: String, c: Connected<'_>) -> Result<(), String> {
    if !c.sites.list().iter().any(|s| s.origin.owns(&url)) {
        return Err("That page isn't on one of your sites.".into());
    }
    open::that_detached(&url).map_err(|e| e.to_string())
}
