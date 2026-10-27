//! The protocol's JSON types (A2.3, A4) and its errors (A6). Unknown
//! response fields are ignored (A11).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_json::value::RawValue;

use crate::http::{NetError, Response};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub handle: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub expires_in: Option<u64>,
    pub refresh_token: String,
    #[serde(default)]
    pub user: Option<User>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeEntry {
    pub id: String,
    pub version: u32,
    pub title: String,
    pub state: String,
    #[serde(default)]
    pub opens_at: Option<String>,
    #[serde(default)]
    pub closes_at: Option<String>,
    pub definition_sha256: String,
    /// The definition's bytes as the site sent them.
    pub definition: Box<RawValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssuedAttempt {
    pub id: String,
    /// The marker ref (A7): opaque, not secret.
    #[serde(rename = "ref")]
    pub marker_ref: String,
    pub challenge_id: String,
    pub challenge_version: u32,
    pub definition_sha256: String,
    pub issued_at: String,
    pub fly_within_s: u64,
    pub upload_within_s: u64,
    #[serde(default)]
    pub fly_by: Option<String>,
    #[serde(default)]
    pub upload_by: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reject {
    pub code: String,
    #[serde(default)]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttemptStatus {
    pub id: String,
    #[serde(default)]
    pub challenge_id: String,
    #[serde(default)]
    pub challenge_version: u32,
    pub status: String,
    #[serde(default)]
    pub issued_at: Option<String>,
    #[serde(default)]
    pub uploaded_at: Option<String>,
    #[serde(default)]
    pub scored_at: Option<String>,
    /// The server's `fdm challenge` output.
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub verified: bool,
    #[serde(default)]
    pub unverified: Vec<String>,
    #[serde(default)]
    pub reject: Option<Reject>,
    #[serde(default)]
    pub rank: Option<u32>,
    #[serde(default)]
    pub url: Option<String>,
}

impl AttemptStatus {
    /// Final statuses (A4); an unknown one is not final.
    pub fn is_final(&self) -> bool {
        matches!(
            self.status.as_str(),
            "scored" | "rejected" | "failed" | "expired" | "abandoned" | "removed"
        )
    }

    /// The server's total score, when scored.
    pub fn score(&self) -> Option<f64> {
        self.result.as_ref()?.get("score")?.get("total")?.as_f64()
    }
}

/// A non-2xx answer: `{"error","message"}` (A6).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ApiError {
    pub http: u16,
    pub code: String,
    pub message: String,
    pub retry_after: Option<u64>,
    pub request_id: Option<String>,
}

impl ApiError {
    pub fn from_response(r: &Response) -> Self {
        #[derive(Deserialize)]
        struct Body {
            #[serde(default)]
            error: String,
            #[serde(default)]
            message: String,
            #[serde(default)]
            retry_after: Option<u64>,
        }
        let body: Option<Body> = serde_json::from_slice(&r.body).ok();
        let (code, message, retry) = body.map_or((String::new(), String::new(), None), |b| {
            (b.error, b.message, b.retry_after)
        });
        Self {
            http: r.status,
            code,
            message: message.chars().take(300).collect(),
            retry_after: r.retry_after.or(retry),
            request_id: r.request_id.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// No answer: offline, DNS, TLS, timeout.
    Network(String),
    Api(ApiError),
    /// No grant, or the refresh token was refused: sign in again.
    SignedOut,
    /// An answer that breaks the protocol (bad JSON, wrong origin, …).
    Protocol(String),
}

impl From<NetError> for Error {
    fn from(e: NetError) -> Self {
        Self::Network(e.0)
    }
}

impl Error {
    /// The error code, or `network` / `signed_out` / `protocol`.
    pub fn code(&self) -> &str {
        match self {
            Self::Network(_) => "network",
            Self::Api(e) => &e.code,
            Self::SignedOut => "signed_out",
            Self::Protocol(_) => "protocol",
        }
    }
}

/// Decodes a 2xx body, or the error.
pub fn decode<T: serde::de::DeserializeOwned>(r: Response) -> Result<T, Error> {
    if !r.ok() {
        return Err(Error::Api(ApiError::from_response(&r)));
    }
    serde_json::from_slice(&r.body).map_err(|e| Error::Protocol(format!("unreadable answer: {e}")))
}
