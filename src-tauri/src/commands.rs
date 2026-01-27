use tauri::State;

use crate::state::{AppState, AuthState};
use crate::app::{dirs, sync};
use crate::settings;

#[derive(serde::Serialize)]
pub struct AuthStatus { pub state: String }

fn map_err(e: anyhow::Error) -> String { format!("{e:#}") }

async fn ensure_storage_chat_id(state: &AppState) -> anyhow::Result<i64> {
  let db = state.db()?;
  let pool = db.pool();

  if let Some(v) = sync::get_sync(pool, "storage_chat_id").await? {
    if let Ok(id) = v.parse::<i64>() { return Ok(id); }
  }

  let tg = state.telegram()?;
  let chat_id = tg.storage_get_or_create_channel().await?;
  sync::set_sync(pool, "storage_chat_id", &chat_id.to_string()).await?;
  Ok(chat_id)
}

#[tauri::command]
pub async fn auth_status(state: State<'_, AppState>) -> Result<AuthStatus, String> {
  let s = match state.auth_state() {
    AuthState::Unknown => "unknown",
    AuthState::WaitConfig => "wait_config",
    AuthState::WaitPhone => "wait_phone",
    AuthState::WaitCode => "wait_code",
    AuthState::WaitPassword => "wait_password",
    AuthState::Ready => "ready",
    AuthState::Closed => "closed"
  };
  Ok(AuthStatus { state: s.to_string() })
}

#[tauri::command]
pub async fn auth_start(state: State<'_, AppState>, phone: String) -> Result<(), String> {
  let tg = state.telegram().map_err(map_err)?;
  tg.auth_start(phone).await.map_err(|e| e.to_string())?;
  Ok(())
}

#[tauri::command]
pub async fn auth_submit_code(state: State<'_, AppState>, code: String) -> Result<(), String> {
  let tg = state.telegram().map_err(map_err)?;
  tg.auth_submit_code(code).await.map_err(|e| e.to_string())?;
  Ok(())
}

#[tauri::command]
pub async fn auth_submit_password(state: State<'_, AppState>, password: String) -> Result<(), String> {
  let tg = state.telegram().map_err(map_err)?;
  tg.auth_submit_password(password).await.map_err(|e| e.to_string())?;
  Ok(())
}

#[tauri::command]
pub async fn storage_get_or_create_channel(state: State<'_, AppState>) -> Result<i64, String> {
  ensure_storage_chat_id(&state).await.map_err(map_err)
}

#[tauri::command]
pub async fn dir_create(state: State<'_, AppState>, parent_id: Option<String>, name: String) -> Result<String, String> {
  let db = state.db().map_err(map_err)?;
  let tg = state.telegram().map_err(map_err)?;
  let chat_id = ensure_storage_chat_id(&state).await.map_err(map_err)?;
  dirs::create_dir(db.pool(), tg.as_ref(), chat_id, parent_id, name).await.map_err(map_err)
}

#[tauri::command]
pub async fn dir_list_tree(state: State<'_, AppState>) -> Result<crate::app::models::DirNode, String> {
  let db = state.db().map_err(map_err)?;
  dirs::list_tree(db.pool()).await.map_err(map_err)
}

#[tauri::command]
pub async fn settings_get_tg(state: State<'_, AppState>) -> Result<settings::TgSettingsView, String> {
  let db = state.db().map_err(map_err)?;
  settings::get_tg_settings_view(db.pool()).await.map_err(map_err)
}

#[tauri::command]
pub async fn settings_set_tg(state: State<'_, AppState>, api_id: i32, api_hash: String) -> Result<(), String> {
  if api_id <= 0 {
    return Err("API_ID должен быть положительным числом".into());
  }
  if api_hash.trim().is_empty() {
    return Err("API_HASH не может быть пустым".into());
  }

  let db = state.db().map_err(map_err)?;
  settings::set_tg_settings(db.pool(), api_id, api_hash.clone()).await.map_err(map_err)?;

  let tg = state.telegram().map_err(map_err)?;
  tg.configure(api_id, api_hash).await.map_err(|e| e.to_string())?;
  Ok(())
}
