pub mod identity;
pub mod knowledge;
pub mod memos;
pub mod moment;

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::time::Duration;

pub(crate) const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const VESPER_API: &str = "https://noted-this.date/api";

/// A Vesper resource URL whose ID is encoded as a single path segment.
fn resource_url(collection: &str, id: &str) -> Result<reqwest::Url, ApiError> {
    if id.trim().is_empty() {
        return Err(ApiError::Protocol("an ID is required".to_owned()));
    }
    let mut url = reqwest::Url::parse(&format!("{VESPER_API}/{collection}/"))
        .map_err(|_| ApiError::Protocol("the Vesper endpoint is invalid".to_owned()))?;
    url.path_segments_mut()
        .map_err(|_| ApiError::Protocol("the Vesper endpoint cannot contain an ID".to_owned()))?
        .pop_if_empty()
        .push(id);
    Ok(url)
}

/// An authenticated HTTP client for one consumer service.
struct Client {
    token: String,
    http: reqwest::Client,
}

impl Client {
    async fn load() -> Result<Self, ApiError> {
        Ok(Self {
            token: identity::access_token().await?,
            http: reqwest::Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .user_agent(concat!("Vesper/", env!("CARGO_PKG_VERSION"), " consumers"))
                .build()?,
        })
    }
}

/// Send a request and require a successful status; callers keep their own
/// success-code assertions and response envelopes.
async fn send(
    request: reqwest::RequestBuilder,
    operation: &'static str,
) -> Result<reqwest::Response, ApiError> {
    let response = request.send().await?;
    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        identity::invalidate().await;
    }
    if !status.is_success() {
        return Err(ApiError::Status { operation, status });
    }
    Ok(response)
}

#[derive(Debug)]
pub enum ApiError {
    Credentials(vault::Error),
    SignedOut,
    Identity(oauth::Error),
    Media(moment::MediaError),
    Request(reqwest::Error),
    Status {
        operation: &'static str,
        status: reqwest::StatusCode,
    },
    Rejected {
        operation: &'static str,
        message: String,
    },
    Protocol(String),
}

impl Display for ApiError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Credentials(source) => Display::fmt(source, formatter),
            Self::SignedOut => write!(formatter, "sign in with your Vesper account"),
            Self::Identity(source) => write!(formatter, "Vesper sign-in failed: {source}"),
            Self::Media(source) => Display::fmt(source, formatter),
            Self::Request(source) => write!(formatter, "consumer API request failed: {source}"),
            Self::Status { operation, status } => {
                write!(formatter, "consumer API {operation} returned {status}")
            }
            Self::Rejected { operation, message } => {
                write!(formatter, "consumer API {operation} rejected: {message}")
            }
            Self::Protocol(message) => {
                write!(formatter, "consumer API returned invalid data: {message}")
            }
        }
    }
}

impl Error for ApiError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Credentials(source) => Some(source),
            Self::Request(source) => Some(source),
            Self::Media(source) => Some(source),
            Self::Identity(source) => Some(source),
            Self::SignedOut | Self::Status { .. } | Self::Rejected { .. } | Self::Protocol(..) => {
                None
            }
        }
    }
}

impl From<vault::Error> for ApiError {
    fn from(source: vault::Error) -> Self {
        Self::Credentials(source)
    }
}

impl From<reqwest::Error> for ApiError {
    fn from(source: reqwest::Error) -> Self {
        Self::Request(source)
    }
}
