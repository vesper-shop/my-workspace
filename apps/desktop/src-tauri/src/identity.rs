use crate::content::Content;
use crate::{CommandError, CommandResponse};
use tauri::Manager;
use tauri_plugin_opener::OpenerExt;

#[tauri::command]
pub(crate) async fn sign_in(app: tauri::AppHandle) -> CommandResponse<String> {
    let result = async {
        let authorization = consumers::api::identity::authorization().await?;
        app.opener()
            .open_url(&authorization.url, None::<String>)
            .map_err(|error| format!("Could not open you-find.me sign-in: {error}"))?;
        let account = consumers::api::identity::sign_in(authorization).await?;
        if let Some(window) = app.get_webview_window("main") {
            window.set_focus()?;
        }
        app.state::<Content>().reset_views().await;
        Ok::<_, CommandError>(account.email)
    };
    result.await.into()
}

#[tauri::command]
pub(crate) async fn sign_out(app: tauri::AppHandle) -> CommandResponse<String> {
    let result = async {
        consumers::api::identity::sign_out().await?;
        app.state::<Content>().reset_views().await;
        Ok::<_, CommandError>("account".to_owned())
    };
    result.await.into()
}
