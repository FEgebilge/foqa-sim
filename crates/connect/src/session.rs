//! One site's grant and its API (A2.3, A4). The access token lives in
//! memory; the refresh token in the credential store (or, when that
//! fails, in memory for this run only). Refresh is single-flight: callers
//! that find the token stale wait for the one refresh in progress, and the
//! rotated refresh token is stored before the new access token is used.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::value::RawValue;
use serde_json::{Value, json};

use crate::api::{self, AttemptStatus, ChallengeEntry, Error, IssuedAttempt, TokenResponse, User};
use crate::http::{Body, Http, NetError, Response};
use crate::store::SecretStore;
use crate::{CLIENT_ID, lock};

/// Refreshed this long before it expires.
const EARLY: Duration = Duration::from_secs(30);

/// A grant event for the connect log (never a token).
pub type Log = Arc<dyn Fn(Value) + Send + Sync>;

pub struct Session {
    http: Arc<Http>,
    store: Arc<dyn SecretStore>,
    origin: String,
    api: String,
    access: Mutex<Option<(String, Instant)>>,
    /// Held for the length of one refresh.
    gate: Mutex<()>,
    /// Bumped on every new access token.
    generation: AtomicU64,
    /// The refresh token when the credential store failed.
    memory: Mutex<Option<String>>,
    log: Log,
}

impl Session {
    pub fn new(
        http: Arc<Http>,
        store: Arc<dyn SecretStore>,
        origin: &str,
        api: &str,
        log: Log,
    ) -> Self {
        Self {
            http,
            store,
            origin: origin.into(),
            api: api.trim_end_matches('/').into(),
            access: Mutex::new(None),
            gate: Mutex::new(()),
            generation: AtomicU64::new(0),
            memory: Mutex::new(None),
            log,
        }
    }

    pub fn api(&self) -> &str {
        &self.api
    }

    pub fn http(&self) -> &Http {
        &self.http
    }

    /// Holds a grant: an access token now or a refresh token kept.
    pub fn has_grant(&self) -> bool {
        lock(&self.access).is_some() || self.refresh_token().is_some()
    }

    /// Takes a new pair; `Ok(false)` when the credential store failed and
    /// the refresh token is only kept until the app quits.
    pub fn accept(&self, t: &TokenResponse) -> Result<bool, Error> {
        let stored = match self.store.set(&self.origin, &t.refresh_token) {
            Ok(()) => {
                *lock(&self.memory) = None;
                true
            }
            Err(e) => {
                (self.log)(
                    json!({ "event": "store_unavailable", "origin": self.origin, "error": e }),
                );
                *lock(&self.memory) = Some(t.refresh_token.clone());
                false
            }
        };
        let ttl = Duration::from_secs(t.expires_in.unwrap_or(3600).min(3600));
        *lock(&self.access) = Some((t.access_token.clone(), Instant::now() + ttl));
        self.generation.fetch_add(1, Ordering::SeqCst);
        Ok(stored)
    }

    fn refresh_token(&self) -> Option<String> {
        let memory = lock(&self.memory).clone();
        memory.or_else(|| self.store.get(&self.origin).ok().flatten())
    }

    /// A usable access token and the generation it belongs to.
    fn token(&self) -> Result<(String, u64), Error> {
        let seen = self.generation.load(Ordering::SeqCst);
        let fresh = lock(&self.access)
            .as_ref()
            .filter(|(_, until)| Instant::now() + EARLY < *until)
            .map(|(t, _)| t.clone());
        match fresh {
            Some(t) => Ok((t, seen)),
            None => self
                .refresh(seen)
                .map(|t| (t, self.generation.load(Ordering::SeqCst))),
        }
    }

    /// Refreshes unless another caller already did since generation
    /// `seen`, in which case its token is returned.
    pub fn refresh(&self, seen: u64) -> Result<String, Error> {
        let _one = lock(&self.gate);
        if self.generation.load(Ordering::SeqCst) != seen
            && let Some((t, _)) = lock(&self.access).clone()
        {
            return Ok(t);
        }
        let token = self.refresh_token().ok_or(Error::SignedOut)?;
        let body = json!({ "grant_type": "refresh_token", "refresh_token": token, "client_id": CLIENT_ID });
        let r = self
            .http
            .post_json(&format!("{}/token", self.api), None, &body)?;
        let request_id = r.request_id.clone();
        match api::decode::<TokenResponse>(r) {
            Ok(t) => {
                (self.log)(
                    json!({ "event": "refresh", "origin": self.origin, "outcome": "ok", "request_id": request_id }),
                );
                self.accept(&t)?;
                Ok(t.access_token)
            }
            Err(Error::Api(e)) if matches!(e.http, 400 | 401) => {
                (self.log)(
                    json!({ "event": "refresh", "origin": self.origin, "outcome": e.code, "http": e.http, "request_id": request_id }),
                );
                self.drop_grant();
                Err(Error::SignedOut)
            }
            Err(e) => Err(e),
        }
    }

    /// Forgets the grant here (memory and credential store).
    pub fn drop_grant(&self) {
        *lock(&self.access) = None;
        *lock(&self.memory) = None;
        if let Err(e) = self.store.delete(&self.origin) {
            (self.log)(json!({ "event": "store_unavailable", "origin": self.origin, "error": e }));
        }
    }

    /// An authenticated call; a 401 refreshes once and tries again, a
    /// second 401 means signed out.
    fn authed(&self, send: impl Fn(&str) -> Result<Response, NetError>) -> Result<Response, Error> {
        let (token, seen) = self.token()?;
        let r = send(&token)?;
        if r.status != 401 {
            return Ok(r);
        }
        let token = self.refresh(seen)?;
        let r = send(&token)?;
        if r.status == 401 {
            *lock(&self.access) = None;
            return Err(Error::SignedOut);
        }
        Ok(r)
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.api)
    }

    pub fn me(&self) -> Result<User, Error> {
        #[derive(Deserialize)]
        struct Me {
            user: User,
        }
        let url = self.url("/me");
        let r = self.authed(|t| self.http.get(&url, Some(t), 64 * 1024))?;
        Ok(api::decode::<Me>(r)?.user)
    }

    /// Open and upcoming challenges.
    pub fn challenges(&self) -> Result<Vec<ChallengeEntry>, Error> {
        #[derive(Deserialize)]
        struct List {
            challenges: Vec<ChallengeEntry>,
        }
        let r = self.http.get(&self.url("/challenges"), None, 1024 * 1024)?;
        Ok(api::decode::<List>(r)?.challenges)
    }

    /// Issues an attempt (idempotent per `client_attempt_id`); returns it
    /// with the definition's bytes as received.
    pub fn issue(
        &self,
        challenge: &str,
        version: u32,
        client_attempt_id: &str,
    ) -> Result<(IssuedAttempt, Box<RawValue>), Error> {
        #[derive(Deserialize)]
        struct Issued {
            attempt: IssuedAttempt,
            definition: Box<RawValue>,
        }
        let url = self.url(&format!("/challenges/{}/attempts", seg(challenge)));
        let body = json!({ "version": version, "client_attempt_id": client_attempt_id });
        let r = self.authed(|t| self.http.post_json(&url, Some(t), &body))?;
        let issued = api::decode::<Issued>(r)?;
        Ok((issued.attempt, issued.definition))
    }

    pub fn abandon(&self, attempt: &str) -> Result<AttemptStatus, Error> {
        let url = self.url(&format!("/attempts/{}/abandon", seg(attempt)));
        let r = self.authed(|t| self.http.send("POST", &url, Some(t), Body::None))?;
        status(r)
    }

    /// `PUT` the recording with its SHA-256.
    pub fn upload(
        &self,
        attempt: &str,
        bytes: &[u8],
        sha256: &str,
    ) -> Result<AttemptStatus, Error> {
        let url = self.url(&format!("/attempts/{}/recording", seg(attempt)));
        let r = self.authed(|t| {
            self.http
                .send("PUT", &url, Some(t), Body::Bytes(bytes, sha256))
        })?;
        status(r)
    }

    /// The app's own grade, display only.
    pub fn client_result(&self, attempt: &str, result: &Value) -> Result<(), Error> {
        let url = self.url(&format!("/attempts/{}/client-result", seg(attempt)));
        let r = self.authed(|t| self.http.send("PUT", &url, Some(t), Body::Json(result)))?;
        if r.ok() {
            Ok(())
        } else {
            Err(Error::Api(api::ApiError::from_response(&r)))
        }
    }

    pub fn attempt(&self, attempt: &str) -> Result<AttemptStatus, Error> {
        let url = self.url(&format!("/attempts/{}", seg(attempt)));
        status(self.authed(|t| self.http.get(&url, Some(t), 256 * 1024))?)
    }

    /// Revokes the grant on the site, then forgets it here whatever the
    /// site answered.
    pub fn revoke(&self) -> Result<(), Error> {
        let url = self.url("/token/revoke");
        let sent = self.authed(|t| self.http.send("POST", &url, Some(t), Body::None));
        self.drop_grant();
        match sent {
            Ok(r) if r.ok() => Ok(()),
            Ok(r) => Err(Error::Api(api::ApiError::from_response(&r))),
            Err(Error::SignedOut) => Ok(()),
            Err(e) => Err(e),
        }
    }
}

fn status(r: Response) -> Result<AttemptStatus, Error> {
    #[derive(Deserialize)]
    struct One {
        attempt: AttemptStatus,
    }
    Ok(api::decode::<One>(r)?.attempt)
}

/// A path segment, percent-encoded.
fn seg(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::{Mock, Reply, tokens};
    use crate::store::MemoryStore;
    use std::sync::atomic::AtomicU32;
    use std::thread;

    pub fn session(mock: &Mock, store: Arc<dyn SecretStore>) -> Session {
        let api = format!("{}/api", mock.base);
        Session::new(
            Arc::new(Http::new("0.2.0")),
            store,
            &mock.base,
            &api,
            Arc::new(|_| {}),
        )
    }

    /// A site whose access tokens are `access-<n>`; `/me` accepts only the newest.
    fn site(slow_refresh: bool) -> (Mock, Arc<AtomicU32>) {
        let issued = Arc::new(AtomicU32::new(1));
        let n = Arc::clone(&issued);
        let mock = Mock::start(move |req, _| match req.path.as_str() {
            "/api/token" => {
                if slow_refresh {
                    thread::sleep(Duration::from_millis(300));
                }
                let current = format!("refresh-{}", n.load(Ordering::SeqCst));
                if req.json()["refresh_token"] != current.as_str() {
                    return Reply::error(400, "invalid_grant");
                }
                Reply::json(200, tokens(n.fetch_add(1, Ordering::SeqCst) + 1))
            }
            "/api/me" => {
                let want = format!("Bearer access-{}", n.load(Ordering::SeqCst));
                if req.header("authorization") == Some(want.as_str()) {
                    Reply::json(200, json!({ "user": { "id": "u1", "handle": "ege" } }))
                } else {
                    Reply::error(401, "invalid_token")
                }
            }
            _ => Reply::error(404, "not_found"),
        });
        (mock, issued)
    }

    #[test]
    fn a_stale_token_is_refreshed_once_for_concurrent_callers() {
        let (mock, _) = site(true);
        let store = Arc::new(MemoryStore::default());
        store.set(&mock.base, "refresh-1").unwrap();
        let s = session(&mock, store.clone());
        assert!(s.has_grant());
        thread::scope(|scope| {
            for _ in 0..6 {
                scope.spawn(|| assert_eq!(s.me().unwrap().handle.as_deref(), Some("ege")));
            }
        });
        assert_eq!(mock.hits("/api/token"), 1, "single-flight");
        assert_eq!(
            store.get(&mock.base).unwrap().as_deref(),
            Some("refresh-2"),
            "rotated token stored"
        );
    }

    #[test]
    fn a_401_refreshes_once_and_a_refused_refresh_signs_out() {
        let (mock, issued) = site(false);
        let store = Arc::new(MemoryStore::default());
        let s = session(&mock, store.clone());
        assert_eq!(s.me(), Err(Error::SignedOut), "no grant at all");
        s.accept(&serde_json::from_value(tokens(1)).unwrap())
            .unwrap();
        // The site rotated behind our back: /me says 401, refresh fixes it.
        issued.store(1, Ordering::SeqCst);
        store.set(&mock.base, "refresh-1").unwrap();
        *lock(&s.access) = Some(("access-0".into(), Instant::now() + Duration::from_secs(600)));
        assert_eq!(s.me().unwrap().id, "u1");
        assert_eq!(mock.hits("/api/token"), 1);
        // A refresh token the site no longer knows: signed out, credential gone.
        store.set(&mock.base, "refresh-old").unwrap();
        *lock(&s.access) = None;
        assert_eq!(s.me(), Err(Error::SignedOut));
        assert_eq!(store.get(&mock.base).unwrap(), None);
        assert!(!s.has_grant());
    }

    #[test]
    fn an_unavailable_store_keeps_the_refresh_token_in_memory_only() {
        let (mock, _) = site(false);
        let s = session(&mock, Arc::new(MemoryStore::broken()));
        let stored = s
            .accept(&serde_json::from_value(tokens(1)).unwrap())
            .unwrap();
        assert!(!stored && s.has_grant());
        *lock(&s.access) = None;
        assert_eq!(
            s.me().unwrap().handle.as_deref(),
            Some("ege"),
            "refreshed from memory"
        );
    }

    #[test]
    fn issue_keeps_the_definition_bytes_and_revoke_forgets_the_grant() {
        let mock = Mock::start(|req, _| {
            match (req.method.as_str(), req.path.as_str()) {
            ("POST", "/api/challenges/ltfj-06/attempts") => Reply::raw(
                201,
                "application/json",
                br#"{"attempt":{"id":"a1","ref":"Qe3v1hJ0dEa9fP2kLm7xRw","challenge_id":"ltfj-06","challenge_version":1,"definition_sha256":"x","issued_at":"t","fly_within_s":1800,"upload_within_s":86400},"definition":{"id": "ltfj-06",  "version":1}}"#.to_vec(),
            ),
            ("POST", "/api/token/revoke") => Reply::raw(204, "application/json", vec![]),
            _ => Reply::error(404, "not_found"),
        }
        });
        let store = Arc::new(MemoryStore::default());
        let s = session(&mock, store.clone());
        s.accept(&serde_json::from_value(tokens(1)).unwrap())
            .unwrap();
        let (a, def) = s.issue("ltfj-06", 1, "local-1").unwrap();
        assert_eq!((a.id.as_str(), a.marker_ref.len()), ("a1", 22));
        assert_eq!(
            def.get(),
            r#"{"id": "ltfj-06",  "version":1}"#,
            "bytes as received"
        );
        let req = &mock.requests()[0];
        assert_eq!(req.json()["client_attempt_id"], "local-1");
        assert_eq!(req.header("authorization"), Some("Bearer access-1"));
        s.revoke().unwrap();
        assert!(!s.has_grant() && store.get(&mock.base).unwrap().is_none());
        assert_eq!(mock.hits("/api/token/revoke"), 1);
    }
}
