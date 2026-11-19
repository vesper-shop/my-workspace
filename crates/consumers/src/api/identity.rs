use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use vault::Stored;

use super::{ApiError, REQUEST_TIMEOUT};

pub use oauth::Authorization;

const AUTHORIZE_ENDPOINT: &str = "https://noted-this.date/api/auth/oauth2/authorize";
const TOKEN_ENDPOINT: &str = "https://noted-this.date/api/auth/oauth2/token";
const REGISTER_ENDPOINT: &str = "https://noted-this.date/api/auth/oauth2/register";
const REVOKE_ENDPOINT: &str = "https://noted-this.date/api/auth/oauth2/revoke";
const WORKSPACE_ENDPOINT: &str = "https://noted-this.date/api/workspace";
const REDIRECT_URI: &str = "http://127.0.0.1:8793/callback";
const SCOPES: &str = "entries:create entries:read entries:update entries:delete photos:create photos:read tags:read offline_access";
const RESOURCES: [&str; 1] = ["https://noted-this.date/mcp"];
const VAULT_ACCOUNT: &str = "vesper";
const CLIENT_ACCOUNT: &str = "vesper-client";
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

#[derive(Serialize, Deserialize)]
struct Registration {
    client_id: String,
}

#[derive(Deserialize)]
struct Workspace {
    handle: String,
}

#[derive(Deserialize)]
struct Profile {
    name: String,
    image: Option<String>,
}

#[derive(Deserialize)]
struct WorkspaceResponse {
    workspace: Workspace,
    profile: Profile,
}

async fn client_id() -> Result<String, ApiError> {
    // Vesper checks requested scopes against the ones registered, so a new scope needs a new client.
    let account = format!("{CLIENT_ACCOUNT} {SCOPES}");
    if let Stored::Ready(saved) = vault::read_json::<Registration>(&account)? {
        return Ok(saved.client_id);
    }
    let registration: Registration = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()?
        .post(REGISTER_ENDPOINT)
        .json(&serde_json::json!({
            "client_name": "Vesper Workspace",
            "application_type": "native",
            "redirect_uris": [REDIRECT_URI],
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "token_endpoint_auth_method": "none",
            "scope": SCOPES,
        }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    vault::save_json(&account, &registration)?;
    Ok(registration.client_id)
}

pub async fn authorization() -> Result<Authorization, ApiError> {
    oauth::authorize(
        &client_id().await?,
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
    let refresh_token = token
        .refresh_token
        .ok_or_else(|| ApiError::Protocol("Vesper did not return a refresh token".to_owned()))?;
    let workspace: WorkspaceResponse = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()?
        .get(WORKSPACE_ENDPOINT)
        .bearer_auth(&token.access_token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let account = Account {
        email: format!("@{}", workspace.workspace.handle),
        name: workspace.profile.name,
        picture: workspace.profile.image,
    };
    let mut access = ACCESS.lock().await;
    vault::save_json(
        VAULT_ACCOUNT,
        &Session {
            refresh_token,
            account: account.clone(),
        },
    )?;
    *access = Some(Access {
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
    let stored = {
        let mut access = ACCESS.lock().await;
        let stored = vault::read_json::<Session>(VAULT_ACCOUNT)?;
        vault::delete(VAULT_ACCOUNT)?;
        *access = None;
        stored
    };
    let Stored::Ready(session) = stored else {
        return Ok(());
    };
    reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()?
        .post(REVOKE_ENDPOINT)
        .form(&[
            ("token", session.refresh_token.as_str()),
            ("token_type_hint", "refresh_token"),
            ("client_id", client_id().await?.as_str()),
        ])
        .send()
        .await?
        .error_for_status()?;
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
    let token = match oauth::refresh(
        &client_id().await?,
        TOKEN_ENDPOINT,
        &session.refresh_token,
        &RESOURCES,
    )
    .await
    {
        Ok(token) => token,
        Err(oauth::Error::InvalidGrant) => {
            vault::delete(VAULT_ACCOUNT)?;
            return Err(ApiError::SignedOut);
        }
        Err(error) => return Err(ApiError::Identity(error)),
    };
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

#[cfg(test)]
#[path = "../../tests/unit/identity.rs"]
mod tests;
