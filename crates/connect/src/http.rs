//! HTTP for the protocol: `ureq` (blocking, rustls with ring, no async
//! runtime) with A4's headers, 10 s to connect, 60 s a request, redirects
//! off (a redirect to another origin would carry the token) and every
//! status returned as a response, not an error.

use std::time::Duration;

use serde::Serialize;
use ureq::Agent;

/// The largest response body read; a well-known document is capped lower.
const MAX_BODY: u64 = 1024 * 1024;

pub struct Http {
    agent: Agent,
    version: String,
}

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
    pub request_id: Option<String>,
    /// `Retry-After` in seconds.
    pub retry_after: Option<u64>,
    pub content_type: Option<String>,
}

impl Response {
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// The request never got an answer: no network, DNS, TLS, timeout.
#[derive(Debug, Clone, PartialEq)]
pub struct NetError(pub String);

pub enum Body<'a> {
    None,
    Json(&'a serde_json::Value),
    /// The recording, with its `X-Foqa-Sha256`.
    Bytes(&'a [u8], &'a str),
}

impl Http {
    /// `app_version` goes in `User-Agent` and `X-Foqa-Version`.
    pub fn new(app_version: &str) -> Self {
        let agent: Agent = Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_global(Some(Duration::from_secs(60)))
            .max_redirects(0)
            .http_status_as_error(false)
            .user_agent(format!("FOQA-Sim/{app_version}"))
            .build()
            .into();
        Self {
            agent,
            version: app_version.to_owned(),
        }
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn get(&self, url: &str, bearer: Option<&str>, limit: u64) -> Result<Response, NetError> {
        let req = self.agent.get(url);
        let req = headers(req, &self.version, bearer);
        read(req.call(), limit)
    }

    pub fn send(
        &self,
        method: &str,
        url: &str,
        bearer: Option<&str>,
        body: Body,
    ) -> Result<Response, NetError> {
        let req = match method {
            "PUT" => self.agent.put(url),
            _ => self.agent.post(url),
        };
        let req = headers(req, &self.version, bearer);
        let sent = match body {
            Body::None => req.send_empty(),
            Body::Json(v) => req.send_json(v),
            Body::Bytes(bytes, sha) => req
                .header("Content-Type", "application/octet-stream")
                .header("X-Foqa-Sha256", sha)
                .send(bytes),
        };
        read(sent, MAX_BODY)
    }

    pub fn post_json(
        &self,
        url: &str,
        bearer: Option<&str>,
        body: &impl Serialize,
    ) -> Result<Response, NetError> {
        let value = serde_json::to_value(body).map_err(|e| NetError(e.to_string()))?;
        self.send("POST", url, bearer, Body::Json(&value))
    }
}

fn headers<B>(
    req: ureq::RequestBuilder<B>,
    version: &str,
    bearer: Option<&str>,
) -> ureq::RequestBuilder<B> {
    let req = req
        .header("Accept", "application/json")
        .header("X-Foqa-Protocol", crate::PROTOCOL.to_string())
        .header("X-Foqa-Version", version);
    match bearer {
        Some(token) => req.header("Authorization", format!("Bearer {token}")),
        None => req,
    }
}

fn read(
    sent: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
    limit: u64,
) -> Result<Response, NetError> {
    let mut res = sent.map_err(|e| NetError(e.to_string()))?;
    let header = |name: &str| {
        res.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    };
    let (request_id, content_type) = (header("x-request-id"), header("content-type"));
    let retry_after = header("retry-after").and_then(|v| v.trim().parse().ok());
    let status = res.status().as_u16();
    let body = res
        .body_mut()
        .with_config()
        .limit(limit)
        .read_to_vec()
        .map_err(|e| NetError(e.to_string()))?;
    Ok(Response {
        status,
        body,
        request_id,
        retry_after,
        content_type,
    })
}
