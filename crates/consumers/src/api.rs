pub mod identity;
pub mod knowledge;
pub mod memos;
pub mod moment;

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::time::Duration;

pub(crate) const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

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
    Store(cms::r2::StoreError),
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
            Self::SignedOut => write!(formatter, "sign in with your you-find.me account"),
            Self::Identity(source) => write!(formatter, "you-find.me sign-in failed: {source}"),
            Self::Media(source) => Display::fmt(source, formatter),
            Self::Store(source) => Display::fmt(source, formatter),
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
            Self::Store(source) => Some(source),
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

impl From<cms::r2::StoreError> for ApiError {
    fn from(source: cms::r2::StoreError) -> Self {
        Self::Store(source)
    }
}

impl From<reqwest::Error> for ApiError {
    fn from(source: reqwest::Error) -> Self {
        Self::Request(source)
    }
}
