//! The `connect` feature (docs/challenge-protocol.md B1): the site client
//! joined to the app. Sites settings, challenges taken from a site, issue
//! before placing, the background uploader and its Tauri commands. The app
//! builds and runs without this module.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use ::connect::api::Error;
use ::connect::copy::{self, Ctx};
use ::connect::http::Http;
use ::connect::sites::{Flow, SiteRecord, Sites};
use ::connect::store::KeyringStore;
use ::connect::uploader::{self, Uploader};
use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Emitter, State};

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
        let uploader = Uploader::new(Arc::clone(&sites), attempts)
            .on_change(move |id| emit(&handle, json!({ "attempt": id })));
        let (tx, rx) = mpsc::channel();
        let connect = Arc::new(Self {
            sites,
            uploader,
            wake: Mutex::new(tx),
            device: Mutex::new(HashMap::new()),
            app,
        });
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
    /// without a slice is refused), abandoned → tell the site.
    pub fn attempt_changed(&self, record: &Record) {
        match record.state {
            AttemptState::Sliced | AttemptState::Done => self.uploader.core_done(&record.id),
            AttemptState::Abandoned | AttemptState::FailedStart => {
                self.uploader.core_abandoned(&record.id)
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
            handle: r.user.as_ref().map(|u| u.handle.clone()),
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
}

impl AttemptHooks for Connect {
    fn before_place(&self, record: &Record) -> Option<String> {
        self.uploader
            .issue(&record.id, &record.key, &record.definition_sha256)
    }

    fn ranking(&self, key: &str) -> Option<(&'static str, String, Option<String>)> {
        let (site, _) = self.sites.source(key)?;
        let label = site.label();
        Some(
            match (&site.user, self.sites.signed_in(site.origin.as_str())) {
                (Some(user), true) => {
                    ("ok", format!("Ranked on {label} as {}.", user.handle), None)
                }
                _ => (
                    "warn",
                    format!(
                        "Signed out of {}: this attempt will be graded here but won't appear on the leaderboard.",
                        site.origin.host()
                    ),
                    Some("Sign in under Sites, or fly it as practice.".into()),
                ),
            },
        )
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
