//! A site for tests: a hand-rolled HTTP/1.1 server on 127.0.0.1 that
//! answers each request with a closure and keeps every request it saw.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use serde_json::{Value, json};

use crate::lock;

#[derive(Debug, Clone)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        let found = self
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name));
        found.map(|(_, v)| v.as_str())
    }

    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
}

pub struct Reply {
    status: u16,
    content_type: String,
    body: Vec<u8>,
    headers: Vec<(String, String)>,
}

impl Reply {
    pub fn json(status: u16, v: Value) -> Self {
        Self::raw(status, "application/json", v.to_string().into_bytes())
    }

    pub fn raw(status: u16, content_type: &str, body: Vec<u8>) -> Self {
        Self {
            status,
            content_type: content_type.into(),
            body,
            headers: vec![("X-Request-Id".into(), "req-1".into())],
        }
    }

    /// `{"error","message"}` (A6).
    pub fn error(status: u16, code: &str) -> Self {
        Self::json(
            status,
            json!({ "error": code, "message": format!("site says {code}") }),
        )
    }

    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
}

type Handler = dyn Fn(&Request, &str) -> Reply + Send + Sync;

pub struct Mock {
    /// `http://127.0.0.1:<port>`.
    pub base: String,
    log: Arc<Mutex<Vec<Request>>>,
    stop: Arc<AtomicBool>,
}

impl Mock {
    /// Serves until dropped; `handler` gets each request and the base URL.
    pub fn start(handler: impl Fn(&Request, &str) -> Reply + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let log = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let handler: Arc<Handler> = Arc::new(handler);
        let (log2, stop2, base2) = (Arc::clone(&log), Arc::clone(&stop), base.clone());
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                if stop2.load(Ordering::SeqCst) {
                    break;
                }
                let (handler, log, base) = (Arc::clone(&handler), Arc::clone(&log2), base2.clone());
                thread::spawn(move || serve(stream, &*handler, &log, &base));
            }
        });
        Self { base, log, stop }
    }

    pub fn requests(&self) -> Vec<Request> {
        lock(&self.log).clone()
    }

    /// Requests to `path`.
    pub fn hits(&self, path: &str) -> usize {
        lock(&self.log)
            .iter()
            .filter(|r| r.path.ends_with(path))
            .count()
    }
}

impl Drop for Mock {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.base.trim_start_matches("http://"));
    }
}

fn serve(stream: TcpStream, handler: &Handler, log: &Mutex<Vec<Request>>, base: &str) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    if reader.read_line(&mut line).unwrap_or(0) == 0 {
        return;
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let target = parts.next().unwrap_or_default().to_owned();
    let mut headers = Vec::new();
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).unwrap_or(0) == 0 || h.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.push((k.trim().to_owned(), v.trim().to_owned()));
        }
    }
    let len = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0; len];
    let _ = reader.read_exact(&mut body);
    let path = target.split_once('?').map_or(target.as_str(), |(p, _)| p);
    let req = Request {
        method,
        path: path.to_owned(),
        headers,
        body,
    };
    lock(log).push(req.clone());
    let reply = handler(&req, base);
    let mut out = stream;
    let mut head = format!(
        "HTTP/1.1 {} X\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n",
        reply.status,
        reply.content_type,
        reply.body.len()
    );
    for (k, v) in &reply.headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    let _ = out.write_all(head.as_bytes());
    let _ = out.write_all(&reply.body);
}

/// A well-known document for a site at `base`.
pub fn well_known(base: &str) -> Value {
    json!({
        "protocol": "foqa-challenges", "versions": [1], "name": "  Test\u{7}Site  ",
        "api": { "1": format!("{base}/api/foqa/v1") },
        "auth": { "authorize": format!("{base}/#/foqa/authorize"), "device_verification": format!("{base}/#/device") },
        "links": { "account": format!("{base}/#/account") },
        "limits": { "max_upload_bytes": 4096 },
        "unknown": "ignored"
    })
}

/// A token response.
pub fn tokens(n: u32) -> Value {
    json!({
        "access_token": format!("access-{n}"), "token_type": "Bearer", "expires_in": 3600,
        "refresh_token": format!("refresh-{n}"), "user": { "id": "u1", "handle": "ege" }
    })
}
