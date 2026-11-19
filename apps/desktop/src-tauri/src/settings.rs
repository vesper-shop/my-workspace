use crate::{CommandError, CommandResponse};
use tauri::Manager;
use vault::Stored;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Configuration {
    ugos: Saved<ugos::credentials::Credentials>,
    account: Saved<consumers::api::identity::Account>,
    codex_resets: ::todo::Subscription,
    notion_calendar: Saved<::todo::notion::configuration::Configuration>,
    app_lock: Saved<String>,
    app_lock_dev: bool,
    spotify: Saved<String>,
    qq_music: Saved<String>,
    publication: social::Configured,
    telegram: Saved<social::telegram::credentials::Credentials>,
    nowledge: Saved<nowledge::credentials::Credentials>,
}

#[derive(serde::Serialize)]
#[serde(tag = "status", content = "data", rename_all = "camelCase")]
enum Saved<T> {
    Missing,
    Ready(T),
}

impl<T> From<Stored<T>> for Saved<T> {
    fn from(stored: Stored<T>) -> Self {
        match stored {
            Stored::Missing => Self::Missing,
            Stored::Ready(value) => Self::Ready(value),
        }
    }
}

#[tauri::command]
pub(crate) async fn read_configuration(app: tauri::AppHandle) -> CommandResponse<Configuration> {
    let result = async {
        let account = consumers::api::identity::account()?.into();
        let (app_lock, app_lock_dev) = match crate::app_lock::read()? {
            Stored::Ready(password) => (Saved::Ready(password.value), password.development),
            Stored::Missing => (Saved::Missing, false),
        };
        let spotify = match music::spotify::credentials::read()? {
            Stored::Ready(credentials) => {
                Saved::Ready(credentials.web_client_id.unwrap_or_default())
            }
            Stored::Missing => Saved::Missing,
        };
        let qq_music = match music::qq::credentials::read()? {
            Stored::Ready(_) => Saved::Ready("renewable-session".to_owned()),
            Stored::Missing => Saved::Missing,
        };
        Ok::<_, CommandError>(Configuration {
            ugos: ugos::credentials::read()?.into(),
            account,
            codex_resets: app.state::<::todo::Store>().read_codex().await?,
            notion_calendar: ::todo::notion::configuration::read()?.into(),
            app_lock,
            app_lock_dev,
            spotify,
            qq_music,
            publication: social::configured()?,
            telegram: social::telegram::credentials::read()?.into(),
            nowledge: nowledge::credentials::read()?.into(),
        })
    };
    result.await.into()
}

#[tauri::command]
pub(crate) fn save_nowledge_configuration(
    credentials: nowledge::credentials::Credentials,
) -> CommandResponse<String> {
    CommandResponse::from(nowledge::credentials::save(credentials).map(|()| "nowledge".to_owned()))
}

#[tauri::command]
pub(crate) fn save_ugos_configuration(
    username: String,
    password: String,
) -> CommandResponse<String> {
    CommandResponse::from(ugos::configure(username, password).map(|()| "ugos".to_owned()))
}

#[tauri::command]
pub(crate) async fn save_notion_calendar(
    configuration: ::todo::notion::configuration::Configuration,
    app: tauri::AppHandle,
) -> CommandResponse<String> {
    let store = app.state::<::todo::Store>();
    let result = store.configure_notion(configuration).await;
    CommandResponse::from(result.map(|()| "notion-calendar".to_owned()))
}

#[tauri::command]
pub(crate) async fn save_codex_resets(
    configuration: ::todo::Subscription,
    app: tauri::AppHandle,
) -> CommandResponse<String> {
    let store = app.state::<::todo::Store>();
    let result = store.save_codex(configuration).await;
    CommandResponse::from(result.map(|()| "codex-resets".to_owned()))
}
