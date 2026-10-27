//! Sign-in (A2). Primary: the system browser and a loopback redirect
//! (RFC 8252) with PKCE S256 (RFC 7636). Fallback: the device flow
//! (RFC 8628). Both end in the same `TokenResponse`. Nothing here is
//! logged: codes, verifiers and `state` stay in memory.

use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use url::Url;

use crate::CLIENT_ID;
use crate::api::{self, Error, TokenResponse};
use crate::http::Http;

/// How long the loopback listener waits for the browser.
pub const TIMEOUT: Duration = Duration::from_secs(5 * 60);
const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";

#[derive(Debug, Clone, PartialEq)]
pub enum AuthError {
    /// The site said no (`access_denied`, `expired_token`, …).
    Denied(String),
    TimedOut,
    Cancelled,
    /// The listener could not bind or the browser could not be opened.
    Local(String),
    Api(Error),
}

impl From<Error> for AuthError {
    fn from(e: Error) -> Self {
        Self::Api(e)
    }
}

/// `n` random bytes, base64url without padding.
pub fn random(n: usize) -> String {
    let mut bytes = vec![0; n];
    getrandom::fill(&mut bytes).expect("the OS random source");
    URL_SAFE_NO_PAD.encode(bytes)
}

/// A PKCE pair: a 43-character verifier and its S256 challenge.
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    pub fn new() -> Self {
        Self::from_verifier(random(32))
    }

    pub fn from_verifier(verifier: String) -> Self {
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        Self {
            verifier,
            challenge,
        }
    }
}

impl Default for Pkce {
    fn default() -> Self {
        Self::new()
    }
}

/// Equal without telling, by timing, how much matched.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0, |d, (x, y)| d | (x ^ y)) == 0
}

/// The one-shot redirect listener on `127.0.0.1`, a random free port.
pub struct Loopback {
    listener: TcpListener,
    port: u16,
}

impl Loopback {
    pub fn bind() -> io::Result<Self> {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?;
        let port = listener.local_addr()?.port();
        if port < 1024 {
            return Err(io::Error::other("no free port above 1023"));
        }
        listener.set_nonblocking(true)?;
        Ok(Self { listener, port })
    }

    pub fn redirect_uri(&self) -> String {
        format!("http://127.0.0.1:{}/callback", self.port)
    }

    /// Waits for the one valid redirect: `GET /callback`, `Host` exactly
    /// `127.0.0.1:<port>`, `state` equal for `code=` and `error=` alike.
    /// Anything else gets 400 and the wait goes on.
    pub fn wait(
        &self,
        state: &str,
        timeout: Duration,
        cancel: &AtomicBool,
    ) -> Result<String, AuthError> {
        let until = Instant::now() + timeout;
        loop {
            if cancel.load(Ordering::SeqCst) {
                return Err(AuthError::Cancelled);
            }
            if Instant::now() >= until {
                return Err(AuthError::TimedOut);
            }
            match self.listener.accept() {
                Ok((stream, _)) => {
                    if let Some(answer) = self.answer(stream, state) {
                        return answer;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(50));
                }
                Err(e) => return Err(AuthError::Local(e.to_string())),
            }
        }
    }

    /// The sign-in's outcome, or `None` for a request that isn't it.
    fn answer(&self, mut stream: TcpStream, state: &str) -> Option<Result<String, AuthError>> {
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let head = read_head(&mut stream);
        let outcome = self.parse(&head, state);
        let (status, page) = match &outcome {
            Some(Ok(_)) => ("200 OK", DONE_PAGE),
            Some(Err(_)) => ("200 OK", DENIED_PAGE),
            None => ("400 Bad Request", "Bad request"),
        };
        let reply = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nContent-Security-Policy: default-src 'none'\r\nConnection: close\r\n\r\n{page}",
            page.len()
        );
        let _ = stream.write_all(reply.as_bytes());
        outcome
    }

    fn parse(&self, head: &str, state: &str) -> Option<Result<String, AuthError>> {
        let mut lines = head.split("\r\n");
        let mut request = lines.next()?.split(' ');
        let (method, target) = (request.next()?, request.next()?);
        let host = lines.find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case("host").then(|| v.trim())
        })?;
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        if method != "GET" || path != "/callback" || host != format!("127.0.0.1:{}", self.port) {
            return None;
        }
        let params: Vec<(String, String)> = url::form_urlencoded::parse(query.as_bytes())
            .into_owned()
            .collect();
        let one = |key: &str| {
            let mut found = params.iter().filter(|(k, _)| k == key);
            match (found.next(), found.next()) {
                (Some((_, v)), None) => Some(v.as_str()),
                _ => None,
            }
        };
        if !same(one("state")?, state) {
            return None;
        }
        match (one("code"), one("error")) {
            (Some(code), None) if !code.is_empty() => Some(Ok(code.to_owned())),
            (None, Some(error)) => Some(Err(AuthError::Denied(error.to_owned()))),
            _ => None,
        }
    }
}

/// The request line and headers, at most 8 KiB.
fn read_head(stream: &mut TcpStream) -> String {
    let mut head = Vec::new();
    let mut buf = [0; 1024];
    while head.len() < 8192 && !head.windows(4).any(|w| w == b"\r\n\r\n") {
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => head.extend_from_slice(&buf[..n]),
        }
    }
    String::from_utf8_lossy(&head).into_owned()
}

const DONE_PAGE: &str = "<!doctype html><meta charset=utf-8><title>FOQA-Sim</title><p style=\"font:16px system-ui;margin:3em\">Signed in. You can return to FOQA-Sim.";
const DENIED_PAGE: &str = "<!doctype html><meta charset=utf-8><title>FOQA-Sim</title><p style=\"font:16px system-ui;margin:3em\">Sign-in was cancelled. You can return to FOQA-Sim.";

/// The browser address: `authorize` with A2.1's query.
pub fn authorize_url(
    authorize: &str,
    redirect_uri: &str,
    state: &str,
    challenge: &str,
    app_version: &str,
) -> Result<String, String> {
    let mut url = Url::parse(authorize).map_err(|e| e.to_string())?;
    url.query_pairs_mut()
        .append_pair("client_id", CLIENT_ID)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("state", state)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("client_name", &format!("FOQA-Sim {app_version}"));
    Ok(url.into())
}

/// The whole loopback sign-in: bind, open the browser at the site's
/// authorize page with `open`, wait, exchange the code.
pub fn sign_in(
    http: &Http,
    authorize: &str,
    api: &str,
    open: &dyn Fn(&str) -> Result<(), String>,
    cancel: &AtomicBool,
) -> Result<TokenResponse, AuthError> {
    let lo = Loopback::bind().map_err(|e| AuthError::Local(e.to_string()))?;
    let (state, pkce, redirect) = (random(16), Pkce::new(), lo.redirect_uri());
    let url = authorize_url(
        authorize,
        &redirect,
        &state,
        &pkce.challenge,
        http.version(),
    )
    .map_err(AuthError::Local)?;
    open(&url).map_err(AuthError::Local)?;
    let code = lo.wait(&state, TIMEOUT, cancel)?;
    drop(lo);
    Ok(exchange(http, api, &code, &pkce.verifier, &redirect)?)
}

pub fn exchange(
    http: &Http,
    api: &str,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<TokenResponse, Error> {
    let body = json!({
        "grant_type": "authorization_code", "code": code, "code_verifier": verifier,
        "redirect_uri": redirect_uri, "client_id": CLIENT_ID,
    });
    api::decode(http.post_json(&format!("{api}/token"), None, &body)?)
}

/// RFC 8628 §3.2.
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct DeviceCode {
    #[serde(skip_serializing)]
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    #[serde(default)]
    pub verification_uri_complete: Option<String>,
    pub expires_in: u64,
    #[serde(default = "five")]
    pub interval: u64,
}

fn five() -> u64 {
    5
}

pub fn device_start(http: &Http, api: &str) -> Result<DeviceCode, Error> {
    let body =
        json!({ "client_id": CLIENT_ID, "client_name": format!("FOQA-Sim {}", http.version()) });
    api::decode(http.post_json(&format!("{api}/device/code"), None, &body)?)
}

/// Polls `/token` until the user approves, honouring `interval`,
/// `slow_down` (+5 s), `authorization_pending`, `access_denied` and
/// `expired_token` (RFC 8628 §3.5). `sleep` waits between polls.
pub fn device_poll(
    http: &Http,
    api: &str,
    code: &DeviceCode,
    sleep: &dyn Fn(Duration),
    cancel: &AtomicBool,
) -> Result<TokenResponse, AuthError> {
    let body = json!({ "grant_type": DEVICE_GRANT, "device_code": code.device_code, "client_id": CLIENT_ID });
    let (mut interval, mut waited) = (code.interval, 0);
    loop {
        if waited >= code.expires_in {
            return Err(AuthError::Denied("expired_token".into()));
        }
        sleep(Duration::from_secs(interval));
        waited += interval.max(1);
        if cancel.load(Ordering::SeqCst) {
            return Err(AuthError::Cancelled);
        }
        let sent = http
            .post_json(&format!("{api}/token"), None, &body)
            .map_err(Error::from);
        match sent.and_then(api::decode) {
            Ok(tokens) => return Ok(tokens),
            Err(Error::Api(e)) if e.code == "authorization_pending" => {}
            Err(Error::Api(e)) if e.code == "slow_down" => interval += 5,
            Err(Error::Api(e)) if e.http == 400 => return Err(AuthError::Denied(e.code)),
            Err(Error::Network(_)) => {}
            Err(e) => return Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::{Mock, Reply, tokens};
    use std::sync::{Arc, Mutex};

    #[test]
    fn pkce_is_s256_of_a_43_character_verifier() {
        // RFC 7636 appendix B.
        let p = Pkce::from_verifier("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".into());
        assert_eq!(p.challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
        let fresh = Pkce::new();
        assert_eq!(fresh.verifier.len(), 43);
        assert_ne!(fresh.verifier, Pkce::new().verifier);
        assert!(same("abc", "abc") && !same("abc", "abd") && !same("abc", "ab"));
    }

    fn get(port: u16, path: &str, host: &str) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(s, "GET {path} HTTP/1.1\r\nHost: {host}\r\n\r\n").unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        out
    }

    #[test]
    fn the_listener_takes_only_the_redirect_with_its_state_and_host() {
        let lo = Loopback::bind().unwrap();
        assert!(lo.port >= 1024);
        let (port, host) = (lo.port, format!("127.0.0.1:{}", lo.port));
        let probes = thread::spawn(move || {
            let mut answers = vec![
                get(port, "/callback?code=c&state=WRONG", &host),
                get(port, "/callback?error=access_denied&state=WRONG", &host),
                get(port, "/callback?code=c&state=s1", "localhost"),
                get(port, "/other?code=c&state=s1", &host),
                get(port, "/callback?code=c&state=s1&state=s1", &host),
            ];
            answers.push(get(port, "/callback?code=the-code&state=s1", &host));
            answers
        });
        let code = lo.wait("s1", Duration::from_secs(10), &AtomicBool::new(false));
        assert_eq!(code, Ok("the-code".into()));
        let answers = probes.join().unwrap();
        assert!(
            answers[..5].iter().all(|a| a.starts_with("HTTP/1.1 400")),
            "{answers:?}"
        );
        assert!(
            answers[5].contains("You can return to FOQA-Sim") && !answers[5].contains("<script")
        );
    }

    #[test]
    fn a_denial_needs_the_state_too_and_the_wait_can_end() {
        let lo = Loopback::bind().unwrap();
        let (port, host) = (lo.port, format!("127.0.0.1:{}", lo.port));
        thread::spawn(move || get(port, "/callback?error=access_denied&state=s2", &host));
        let denied = lo.wait("s2", Duration::from_secs(10), &AtomicBool::new(false));
        assert_eq!(denied, Err(AuthError::Denied("access_denied".into())));
        let lo = Loopback::bind().unwrap();
        assert_eq!(
            lo.wait("s", Duration::from_millis(100), &AtomicBool::new(false)),
            Err(AuthError::TimedOut)
        );
        assert_eq!(
            lo.wait("s", Duration::from_secs(5), &AtomicBool::new(true)),
            Err(AuthError::Cancelled)
        );
    }

    #[test]
    fn the_browser_sign_in_exchanges_the_code_with_its_verifier() {
        let mock = Mock::start(|req, _| match req.path.as_str() {
            "/api/token" => Reply::json(200, tokens(1)),
            _ => Reply::error(404, "not_found"),
        });
        let api = format!("{}/api", mock.base);
        let authorize = format!("{}/#/foqa/authorize", mock.base);
        // The "browser" follows the redirect with the state it was given.
        let open = |url: &str| {
            let url = Url::parse(url).unwrap();
            let q: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
            assert_eq!(
                (q["client_id"].as_str(), q["code_challenge_method"].as_str()),
                ("foqa-sim", "S256")
            );
            assert_eq!(q["client_name"], "FOQA-Sim 0.2.0");
            let redirect = Url::parse(&q["redirect_uri"]).unwrap();
            let port = redirect.port().unwrap();
            let path = format!("/callback?code=abc&state={}", q["state"]);
            thread::spawn(move || get(port, &path, &format!("127.0.0.1:{port}")));
            Ok(())
        };
        let t = sign_in(
            &Http::new("0.2.0"),
            &authorize,
            &api,
            &open,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(
            (t.access_token.as_str(), t.user.unwrap().handle.as_deref()),
            ("access-1", Some("ege"))
        );
        let body = mock.requests()[0].json();
        assert_eq!(body["grant_type"], "authorization_code");
        assert_eq!(body["code"], "abc");
        let verifier = body["code_verifier"].as_str().unwrap();
        assert_eq!(verifier.len(), 43);
        assert!(
            body["redirect_uri"]
                .as_str()
                .unwrap()
                .starts_with("http://127.0.0.1:")
        );
    }

    #[test]
    fn the_device_flow_waits_slows_down_and_stops_on_denial() {
        let answers = Arc::new(Mutex::new(vec![
            Reply::error(400, "authorization_pending"),
            Reply::error(400, "slow_down"),
            Reply::json(200, tokens(2)),
        ]));
        let a = Arc::clone(&answers);
        let mock = Mock::start(move |req, _| match req.path.as_str() {
            "/api/device/code" => Reply::json(
                200,
                serde_json::json!({
                    "device_code": "dc", "user_code": "WDJB-MJHT", "verification_uri": "https://x/device",
                    "expires_in": 600, "interval": 2
                }),
            ),
            _ => crate::lock(&a).remove(0),
        });
        let (http, api) = (Http::new("0"), format!("{}/api", mock.base));
        let code = device_start(&http, &api).unwrap();
        assert_eq!(code.user_code, "WDJB-MJHT");
        let slept = Mutex::new(Vec::new());
        let sleep = |d: Duration| crate::lock(&slept).push(d.as_secs());
        let t = device_poll(&http, &api, &code, &sleep, &AtomicBool::new(false)).unwrap();
        assert_eq!(t.refresh_token, "refresh-2");
        assert_eq!(*crate::lock(&slept), [2, 2, 7]);
        assert_eq!(mock.requests()[1].json()["grant_type"], DEVICE_GRANT);

        crate::lock(&answers).push(Reply::error(400, "access_denied"));
        let denied = device_poll(&http, &api, &code, &|_| {}, &AtomicBool::new(false));
        assert_eq!(
            denied.unwrap_err(),
            AuthError::Denied("access_denied".into())
        );
    }
}
