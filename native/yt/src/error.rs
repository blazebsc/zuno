use thiserror::Error;

#[derive(Debug, Error)]
pub enum YtError {
    #[error("not signed in (paste a cookie header via Session::import)")]
    NotSignedIn,
    #[error("http: {0}")]
    Http(String),
    #[error("innertube returned HTTP {0}: {1}")]
    Status(u16, String),
    #[error("json: {0}")]
    Json(String),
    #[error("video not playable: {0}")]
    NotPlayable(String),
    #[error("no direct audio stream (all formats ciphered or missing)")]
    NoStream,
    #[error("not found: {0}")]
    NotFound(String),
    #[error("parse: {0}")]
    Parse(String),
}

impl From<reqwest::Error> for YtError {
    fn from(e: reqwest::Error) -> Self {
        YtError::Http(e.to_string())
    }
}

impl From<serde_json::Error> for YtError {
    fn from(e: serde_json::Error) -> Self {
        YtError::Json(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, YtError>;
