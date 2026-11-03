use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use vault::Stored;

use super::{ApiError, REQUEST_TIMEOUT};

pub use oauth::Authorization;

const AUTHORIZE_ENDPOINT: &str = "https://you-find.me/api/auth/oauth2/authorize";
const TOKEN_ENDPOINT: &str = "https://you-find.me/api/auth/oauth2/token";
const USERINFO_ENDPOINT: &str = "https://you-find.me/api/auth/oauth2/userinfo";
const REVOKE_ENDPOINT: &str = "https://you-find.me/api/auth/oauth2/revoke";
const CLIENT_ID: &str = "workspace";
const REDIRECT_URI: &str = "http://127.0.0.1:8793/callback";
const SCOPES: &str = "openid profile email offline_access memos:read memos:write moment:read moment:write knowledge:read knowledge:write";
const RESOURCES: [&str; 3] = [
    "https://memos.you-find.me",
    "https://moment.you-find.me",
    "https://knowledge.you-find.me",
];
const VAULT_ACCOUNT: &str = "you-find-me";
const MARGIN: Duration = Duration::from_secs(60);

#[derive(Clone, Serialize, Deserialize)]
pub struct Account {
    pub email: String,
    pub name: String,
    pub picture: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Session {
    refresh_token: String,
    account: Account,
}

struct Access {
    token: String,
    expires_at: Instant,
}

static ACCESS: Mutex<Option<Access>> = Mutex::const_new(None);

pub async fn authorization() -> Result<Authorization, ApiError> {
    oauth::authorize(
        CLIENT_ID,
        AUTHORIZE_ENDPOINT,
        TOKEN_ENDPOINT,
        REDIRECT_URI,
        SCOPES,
        &RESOURCES,
    )
    .await
    .map_err(ApiError::Identity)
}

pub async fn sign_in(authorization: Authorization) -> Result<Account, ApiError> {
    let token = oauth::authenticate(authorization)
        .await
        .map_err(ApiError::Identity)?;
    let refresh_token = token.refresh_token.ok_or_else(|| {
        ApiError::Protocol("you-find.me did not return a refresh token".to_owned())
    })?;
    let account: Account = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()?
        .get(USERINFO_ENDPOINT)
        .bearer_auth(&token.access_token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    vault::save_json(
        VAULT_ACCOUNT,
        &Session {
            refresh_token,
            account: account.clone(),
        },
    )?;
    *ACCESS.lock().await = Some(Access {
        token: token.access_token,
        expires_at: Instant::now() + Duration::from_secs(token.expires_in),
    });
    Ok(account)
}

pub fn account() -> Result<Stored<Account>, ApiError> {
    Ok(match vault::read_json::<Session>(VAULT_ACCOUNT)? {
        Stored::Ready(session) => Stored::Ready(session.account),
        Stored::Missing => Stored::Missing,
    })
}

pub async fn sign_out() -> Result<(), ApiError> {
    if let Stored::Ready(session) = vault::read_json::<Session>(VAULT_ACCOUNT)? {
        reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()?
            .post(REVOKE_ENDPOINT)
            .form(&[
                ("token", session.refresh_token.as_str()),
                ("token_type_hint", "refresh_token"),
                ("client_id", CLIENT_ID),
            ])
            .send()
            .await?
            .error_for_status()?;
    }
    vault::delete(VAULT_ACCOUNT)?;
    *ACCESS.lock().await = None;
    Ok(())
}

pub(crate) async fn access_token() -> Result<String, ApiError> {
    let mut access = ACCESS.lock().await;
    if let Some(current) = access
        .as_ref()
        .filter(|current| current.expires_at > Instant::now() + MARGIN)
    {
        return Ok(current.token.clone());
    }
    let Stored::Ready(session) = vault::read_json::<Session>(VAULT_ACCOUNT)? else {
        return Err(ApiError::SignedOut);
    };
    let token = oauth::refresh(
        CLIENT_ID,
        TOKEN_ENDPOINT,
        &session.refresh_token,
        &RESOURCES,
    )
    .await
    .map_err(ApiError::Identity)?;
    if let Some(rotated) = token.refresh_token {
        vault::save_json(
            VAULT_ACCOUNT,
            &Session {
                refresh_token: rotated,
                account: session.account,
            },
        )?;
    }
    *access = Some(Access {
        token: token.access_token.clone(),
        expires_at: Instant::now() + Duration::from_secs(token.expires_in),
    });
    Ok(token.access_token)
}

pub(crate) async fn invalidate() {
    *ACCESS.lock().await = None;
}
