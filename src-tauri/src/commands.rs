use tauri::{Emitter, State, AppHandle};
use std::path::{Path, PathBuf};
use sqlx::Row;
use chrono::Utc;
use serde::Deserialize;
use crate::state::{AppState, AuthState};
use crate::app::{backup, dirs, sync, files, indexer, reconcile};
use crate::settings;
use crate::secrets::{self, CredentialsSource};
use crate::paths::Paths;
use crate::fsmeta::{DirMeta, make_dir_message};
use tracing::info;

#[derive(serde::Serialize)]
pub struct AuthStatus { pub state: String }

#[derive(Clone, serde::Serialize)]
pub struct TgSyncStatus {
  pub state: String,
  pub message: String,
  pub processed: i64,
  pub total: Option<i64>
}

#[derive(serde::Serialize)]
pub struct TgCredentialsView {
  pub available: bool,
  pub source: Option<String>,
  pub keychain_available: bool,
  pub encrypted_present: bool,
  pub locked: bool
}

#[derive(serde::Serialize)]
pub struct TgSettingsView {
  pub tdlib_path: Option<String>,
  pub credentials: TgCredentialsView
}

#[derive(serde::Serialize)]
pub struct TgSettingsSaveResult {
  pub storage: Option<String>,
  pub message: String
}

#[derive(serde::Serialize)]
pub struct ChatView {
  pub id: i64,
  pub title: String,
  pub kind: String,
  pub username: Option<String>
}

#[derive(serde::Serialize)]
pub struct ShareResult {
  pub message: String
}

#[derive(serde::Serialize)]
pub struct TgReconcileResult {
  pub message: String,
  pub scanned: i64,
  pub marked: i64,
  pub cleared: i64,
  pub imported: i64
}

#[derive(serde::Serialize)]
pub struct BackupResult {
  pub message: String
}

#[derive(serde::Serialize)]
pub struct RepairResult {
  pub ok: bool,
  pub message: String,
  pub code: Option<String>
}

const RECONCILE_SYNC_REQUIRED: &str = "RECONCILE_SYNC_REQUIRED";
const REPAIR_NEED_FILE: &str = "REPAIR_NEED_FILE";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TgSettingsInput {
  pub api_id: Option<i32>,
  pub api_hash: Option<String>,
  pub remember: Option<bool>,
  pub storage_mode: Option<String>,
  pub password: Option<String>,
  pub tdlib_path: Option<String>
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSearchInput {
  pub dir_id: Option<String>,
  pub name: Option<String>,
  pub file_type: Option<String>,
  pub limit: Option<i64>
}

fn map_err(e: anyhow::Error) -> String { format!("{e:#}") }

fn emit_sync(app: &AppHandle, state: &str, message: &str, processed: i64, total: Option<i64>) {
  let payload = TgSyncStatus {
    state: state.to_string(),
    message: message.to_string(),
    processed,
    total
  };
  let _ = app.emit("tg_sync_status", payload);
}

async fn download_file_path(state: &AppState, file_id: &str, overwrite: bool) -> anyhow::Result<PathBuf> {
  let db = state.db()?;
  let tg = state.telegram()?;
  let paths = state.paths()?;
  let storage_chat_id = ensure_storage_chat_id(state).await?;
  files::download_file(db.pool(), tg.as_ref(), &paths, storage_chat_id, file_id, overwrite).await
}

async fn local_file_path(state: &AppState, file_id: &str) -> anyhow::Result<Option<PathBuf>> {
  let db = state.db()?;
  let paths = state.paths()?;
  files::find_local_download_path(db.pool(), &paths, file_id).await
}

fn open_file_in_os(path: &Path) -> anyhow::Result<()> {
  #[cfg(target_os = "windows")]
  {
    let path_str = path.to_string_lossy().to_string();
    std::process::Command::new("cmd")
      .args(["/C", "start", "", &path_str])
      .spawn()?;
    return Ok(());
  }
  #[cfg(target_os = "macos")]
  {
    let path_str = path.to_string_lossy().to_string();
    std::process::Command::new("open")
      .arg(&path_str)
      .spawn()?;
    return Ok(());
  }
  #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
  {
    std::process::Command::new("xdg-open")
      .arg(path)
      .spawn()?;
    return Ok(());
  }
}

fn open_url_in_os(url: &str) -> anyhow::Result<()> {
  #[cfg(target_os = "windows")]
  {
    std::process::Command::new("explorer")
      .arg(url)
      .spawn()?;
    return Ok(());
  }
  #[cfg(target_os = "macos")]
  {
    std::process::Command::new("open")
      .arg(url)
      .spawn()?;
    return Ok(());
  }
  #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
  {
    std::process::Command::new("xdg-open")
      .arg(url)
      .spawn()?;
    return Ok(());
  }
}

fn open_folder_for_file(path: &Path) -> anyhow::Result<()> {
  let folder = path.parent().unwrap_or_else(|| Path::new("."));
  #[cfg(target_os = "windows")]
  {
    let folder_str = folder.to_string_lossy().to_string();
    std::process::Command::new("explorer")
      .arg(&folder_str)
      .spawn()?;
    return Ok(());
  }
  #[cfg(target_os = "macos")]
  {
    let folder_str = folder.to_string_lossy().to_string();
    std::process::Command::new("open")
      .arg(&folder_str)
      .spawn()?;
    return Ok(());
  }
  #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
  {
    std::process::Command::new("xdg-open")
      .arg(folder)
      .spawn()?;
    return Ok(());
  }
}

async fn ensure_storage_chat_id(state: &AppState) -> anyhow::Result<i64> {
  let db = state.db()?;
  let pool = db.pool();
  let tg = state.telegram()?;
  let mut previous_id: Option<i64> = None;

  if let Some(v) = sync::get_sync(pool, "storage_chat_id").await? {
    if let Ok(id) = v.parse::<i64>() {
      if id == 777 {
        info!(event = "storage_chat_id_invalid", value = v, "Обнаружен mock chat_id, пересоздаю");
      } else {
        previous_id = Some(id);
      }
    } else {
      info!(event = "storage_chat_id_invalid", value = v, "Некорректный storage_chat_id, пересоздаю");
    }
  }

  if let Some(id) = previous_id {
    if tg.storage_check_channel(id).await.unwrap_or(false) {
      info!(event = "storage_chat_id_cached", chat_id = id, "Использую сохраненный storage_chat_id");
      return Ok(id);
    }
    info!(event = "storage_chat_id_invalid", chat_id = id, "Канал хранения недоступен, создаю новый");
  }

  let chat_id = tg.storage_get_or_create_channel().await?;
  sync::set_sync(pool, "storage_chat_id", &chat_id.to_string()).await?;
  info!(event = "storage_chat_id_saved", chat_id = chat_id, "storage_chat_id сохранен");

  let mut reseed_ok = true;
  if previous_id.filter(|id| *id != chat_id).is_some() || previous_id.is_none() {
    if let Err(e) = reseed_storage_channel(pool, tg.as_ref(), previous_id, chat_id).await {
      reseed_ok = false;
      tracing::error!(event = "storage_channel_reseed_failed", error = %e, "Не удалось пересоздать содержимое канала");
    }
  }

  if reseed_ok {
    if let Some(old_id) = previous_id.filter(|id| *id != chat_id) {
      if let Err(e) = tg.storage_delete_channel(old_id).await {
        tracing::warn!(event = "storage_channel_delete_failed", chat_id = old_id, error = %e, "Не удалось удалить старый канал");
      }
    }
  }

  Ok(chat_id)
}

async fn ensure_backup_chat_id(state: &AppState) -> anyhow::Result<i64> {
  let db = state.db()?;
  let pool = db.pool();
  let tg = state.telegram()?;

  if let Some(v) = sync::get_sync(pool, "backup_chat_id").await? {
    if let Ok(id) = v.parse::<i64>() {
      if tg.backup_check_channel(id).await.unwrap_or(false) {
        info!(event = "backup_chat_id_cached", chat_id = id, "Использую сохраненный backup_chat_id");
        return Ok(id);
      }
      info!(event = "backup_chat_id_invalid", chat_id = id, "Канал бэкапов недоступен, создаю новый");
    } else {
      info!(event = "backup_chat_id_invalid", value = v, "Некорректный backup_chat_id, создаю новый");
    }
  }

  let chat_id = tg.backup_get_or_create_channel().await?;
  sync::set_sync(pool, "backup_chat_id", &chat_id.to_string()).await?;
  info!(event = "backup_chat_id_saved", chat_id = chat_id, "backup_chat_id сохранен");
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
  info!(event = "auth_start", phone_masked = %mask_phone(&phone), "Запрос кода авторизации");
  let tg = state.telegram().map_err(map_err)?;
  tg.auth_start(phone).await.map_err(|e| e.to_string())?;
  Ok(())
}

#[tauri::command]
pub async fn auth_submit_code(state: State<'_, AppState>, code: String) -> Result<(), String> {
  info!(event = "auth_submit_code", code_len = code.len(), "Отправка кода авторизации");
  let tg = state.telegram().map_err(map_err)?;
  tg.auth_submit_code(code).await.map_err(|e| e.to_string())?;
  Ok(())
}

#[tauri::command]
pub async fn auth_submit_password(state: State<'_, AppState>, password: String) -> Result<(), String> {
  info!(event = "auth_submit_password", password_len = password.len(), "Отправка пароля 2FA");
  let tg = state.telegram().map_err(map_err)?;
  tg.auth_submit_password(password).await.map_err(|e| e.to_string())?;
  Ok(())
}

#[tauri::command]
pub async fn storage_get_or_create_channel(state: State<'_, AppState>) -> Result<i64, String> {
  info!(event = "storage_get_or_create_channel", "Запрос storage канала");
  ensure_storage_chat_id(&state).await.map_err(map_err)
}

#[tauri::command]
pub async fn dir_create(app: AppHandle, state: State<'_, AppState>, parent_id: Option<String>, name: String) -> Result<String, String> {
  info!(event = "dir_create", parent_id = parent_id.as_deref().unwrap_or("ROOT"), "Создание директории");
  let db = state.db().map_err(map_err)?;
  let tg = state.telegram().map_err(map_err)?;
  let chat_id = ensure_storage_chat_id(&state).await.map_err(map_err)?;
  let id = dirs::create_dir(db.pool(), tg.as_ref(), chat_id, parent_id, name).await.map_err(map_err)?;
  let _ = app.emit("tree_updated", ());
  Ok(id)
}

#[tauri::command]
pub async fn dir_rename(app: AppHandle, state: State<'_, AppState>, dir_id: String, name: String) -> Result<(), String> {
  info!(event = "dir_rename", dir_id = dir_id.as_str(), "Переименование директории");
  if dir_id == "ROOT" {
    return Err("Нельзя переименовать корневую папку".into());
  }
  if name.trim().is_empty() {
    return Err("Имя папки не может быть пустым".into());
  }
  let db = state.db().map_err(map_err)?;
  let tg = state.telegram().map_err(map_err)?;
  let chat_id = ensure_storage_chat_id(&state).await.map_err(map_err)?;
  dirs::rename_dir(db.pool(), tg.as_ref(), chat_id, &dir_id, name).await.map_err(map_err)?;
  let _ = app.emit("tree_updated", ());
  Ok(())
}

#[tauri::command]
pub async fn dir_move(app: AppHandle, state: State<'_, AppState>, dir_id: String, parent_id: Option<String>) -> Result<(), String> {
  info!(event = "dir_move", dir_id = dir_id.as_str(), parent_id = parent_id.as_deref().unwrap_or("ROOT"), "Перемещение директории");
  if dir_id == "ROOT" {
    return Err("Нельзя перемещать корневую папку".into());
  }
  let db = state.db().map_err(map_err)?;
  let tg = state.telegram().map_err(map_err)?;
  let chat_id = ensure_storage_chat_id(&state).await.map_err(map_err)?;
  dirs::move_dir(db.pool(), tg.as_ref(), chat_id, &dir_id, parent_id).await.map_err(map_err)?;
  let _ = app.emit("tree_updated", ());
  Ok(())
}

#[tauri::command]
pub async fn dir_delete(app: AppHandle, state: State<'_, AppState>, dir_id: String) -> Result<(), String> {
  info!(event = "dir_delete", dir_id = dir_id.as_str(), "Удаление директории");
  if dir_id == "ROOT" {
    return Err("Нельзя удалить корневую папку".into());
  }
  let db = state.db().map_err(map_err)?;
  let tg = state.telegram().map_err(map_err)?;
  let chat_id = ensure_storage_chat_id(&state).await.map_err(map_err)?;
  dirs::delete_dir(db.pool(), tg.as_ref(), chat_id, &dir_id).await.map_err(map_err)?;
  let _ = app.emit("tree_updated", ());
  Ok(())
}

#[tauri::command]
pub async fn dir_repair(app: AppHandle, state: State<'_, AppState>, dir_id: String) -> Result<RepairResult, String> {
  info!(event = "dir_repair", dir_id = dir_id.as_str(), "Восстановление директории");
  if dir_id == "ROOT" {
    return Err("Нельзя восстановить корневую папку".into());
  }
  let db = state.db().map_err(map_err)?;
  let tg = state.telegram().map_err(map_err)?;
  let chat_id = ensure_storage_chat_id(&state).await.map_err(map_err)?;
  dirs::repair_dir(db.pool(), tg.as_ref(), chat_id, &dir_id).await.map_err(map_err)?;
  let _ = app.emit("tree_updated", ());
  Ok(RepairResult { ok: true, message: "Папка восстановлена.".to_string(), code: None })
}

#[tauri::command]
pub async fn dir_list_tree(state: State<'_, AppState>) -> Result<crate::app::models::DirNode, String> {
  let db = state.db().map_err(map_err)?;
  dirs::list_tree(db.pool()).await.map_err(map_err)
}

#[tauri::command]
pub async fn file_list(state: State<'_, AppState>, dir_id: String) -> Result<Vec<files::FileItem>, String> {
  let db = state.db().map_err(map_err)?;
  let paths = state.paths().map_err(map_err)?;
  files::list_files(db.pool(), &paths, &dir_id).await.map_err(map_err)
}

#[tauri::command]
pub async fn file_search(state: State<'_, AppState>, input: FileSearchInput) -> Result<Vec<files::FileItem>, String> {
  let db = state.db().map_err(map_err)?;
  let paths = state.paths().map_err(map_err)?;
  files::search_files(
    db.pool(),
    &paths,
    input.dir_id.as_deref(),
    input.name.as_deref(),
    input.file_type.as_deref(),
    input.limit
  )
  .await
  .map_err(map_err)
}

#[tauri::command]
pub async fn file_pick() -> Result<Vec<String>, String> {
  let files = rfd::FileDialog::new().pick_files().unwrap_or_default();
  Ok(files
    .into_iter()
    .map(|p| p.to_string_lossy().to_string())
    .collect())
}

#[tauri::command]
pub async fn file_upload(state: State<'_, AppState>, dir_id: String, path: String) -> Result<String, String> {
  info!(event = "file_upload", dir_id = dir_id.as_str(), "Загрузка файла");
  let db = state.db().map_err(map_err)?;
  let tg = state.telegram().map_err(map_err)?;
  let chat_id = ensure_storage_chat_id(&state).await.map_err(map_err)?;
  let id = files::upload_file(db.pool(), tg.as_ref(), chat_id, &dir_id, Path::new(&path)).await.map_err(map_err)?;
  Ok(id)
}

#[tauri::command]
pub async fn file_move(state: State<'_, AppState>, file_id: String, dir_id: String) -> Result<(), String> {
  info!(event = "file_move", file_id = file_id.as_str(), dir_id = dir_id.as_str(), "Перемещение файла");
  let db = state.db().map_err(map_err)?;
  let tg = state.telegram().map_err(map_err)?;
  let chat_id = ensure_storage_chat_id(&state).await.map_err(map_err)?;
  files::move_file(db.pool(), tg.as_ref(), chat_id, &file_id, &dir_id).await.map_err(map_err)?;
  Ok(())
}

#[tauri::command]
pub async fn file_delete(state: State<'_, AppState>, file_id: String) -> Result<(), String> {
  info!(event = "file_delete", file_id = file_id.as_str(), "Удаление файла");
  let db = state.db().map_err(map_err)?;
  let tg = state.telegram().map_err(map_err)?;
  let paths = state.paths().map_err(map_err)?;
  files::delete_file(db.pool(), tg.as_ref(), &paths, &file_id).await.map_err(map_err)?;
  Ok(())
}

#[tauri::command]
pub async fn file_repair(state: State<'_, AppState>, file_id: String, path: Option<String>) -> Result<RepairResult, String> {
  info!(event = "file_repair", file_id = file_id.as_str(), "Восстановление файла");
  let db = state.db().map_err(map_err)?;
  let tg = state.telegram().map_err(map_err)?;
  let paths = state.paths().map_err(map_err)?;
  let chat_id = ensure_storage_chat_id(&state).await.map_err(map_err)?;
  let path = path.as_deref().map(std::path::Path::new);
  let outcome = files::repair_file(db.pool(), tg.as_ref(), &paths, chat_id, &file_id, path)
    .await
    .map_err(map_err)?;
  match outcome {
    files::RepairFileResult::Repaired => Ok(RepairResult {
      ok: true,
      message: "Файл восстановлен.".to_string(),
      code: None
    }),
    files::RepairFileResult::NeedFile => Ok(RepairResult {
      ok: false,
      message: "Не удалось восстановить файл: нужно выбрать файл для переотправки.".to_string(),
      code: Some(REPAIR_NEED_FILE.to_string())
    })
  }
}

#[tauri::command]
pub async fn file_delete_many(state: State<'_, AppState>, file_ids: Vec<String>) -> Result<(), String> {
  info!(event = "file_delete_many", count = file_ids.len(), "Удаление нескольких файлов");
  let db = state.db().map_err(map_err)?;
  let tg = state.telegram().map_err(map_err)?;
  let paths = state.paths().map_err(map_err)?;
  files::delete_files(db.pool(), tg.as_ref(), &paths, &file_ids).await.map_err(map_err)?;
  Ok(())
}

#[tauri::command]
pub async fn file_download(state: State<'_, AppState>, file_id: String, overwrite: Option<bool>) -> Result<String, String> {
  info!(event = "file_download", file_id = file_id.as_str(), "Скачивание файла");
  let path = download_file_path(&state, &file_id, overwrite.unwrap_or(false)).await.map_err(map_err)?;
  Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn file_open(state: State<'_, AppState>, file_id: String) -> Result<(), String> {
  let path = match local_file_path(&state, &file_id).await.map_err(map_err)? {
    Some(path) => path,
    None => download_file_path(&state, &file_id, false).await.map_err(map_err)?
  };
  open_file_in_os(&path).map_err(map_err)?;
  Ok(())
}

#[tauri::command]
pub async fn file_open_folder(state: State<'_, AppState>, file_id: String) -> Result<(), String> {
  let Some(path) = local_file_path(&state, &file_id).await.map_err(map_err)? else {
    return Err("Файл еще не скачан.".to_string());
  };
  open_folder_for_file(&path).map_err(map_err)?;
  Ok(())
}

#[tauri::command]
pub async fn file_share_link(state: State<'_, AppState>, file_id: String) -> Result<String, String> {
  let db = state.db().map_err(map_err)?;
  let row = sqlx::query("SELECT tg_chat_id, tg_msg_id FROM files WHERE id = ?")
    .bind(&file_id)
    .fetch_optional(db.pool())
    .await
    .map_err(|e| map_err(e.into()))?;
  let Some(row) = row else {
    return Err("Файл не найден".into());
  };
  let chat_id: i64 = row.get("tg_chat_id");
  let msg_id: i64 = row.get("tg_msg_id");
  files::build_message_link(chat_id, msg_id).map_err(map_err)
}

#[tauri::command]
pub async fn tg_search_chats(state: State<'_, AppState>, query: String) -> Result<Vec<ChatView>, String> {
  let tg = state.telegram().map_err(map_err)?;
  let items = tg.search_chats(query, 20).await.map_err(|e| e.to_string())?;
  Ok(items
    .into_iter()
    .map(|c| ChatView {
      id: c.id,
      title: c.title,
      kind: c.kind,
      username: c.username
    })
    .collect())
}

#[tauri::command]
pub async fn tg_recent_chats(state: State<'_, AppState>) -> Result<Vec<ChatView>, String> {
  let tg = state.telegram().map_err(map_err)?;
  let items = tg.recent_chats(12).await.map_err(|e| e.to_string())?;
  Ok(items
    .into_iter()
    .map(|c| ChatView {
      id: c.id,
      title: c.title,
      kind: c.kind,
      username: c.username
    })
    .collect())
}

#[tauri::command]
pub async fn file_share_to_chat(state: State<'_, AppState>, file_id: String, chat_id: i64) -> Result<ShareResult, String> {
  let db = state.db().map_err(map_err)?;
  let tg = state.telegram().map_err(map_err)?;
  let row = sqlx::query("SELECT tg_chat_id, tg_msg_id FROM files WHERE id = ?")
    .bind(&file_id)
    .fetch_optional(db.pool())
    .await
    .map_err(|e| map_err(e.into()))?;
  let Some(row) = row else {
    return Err("Файл не найден".into());
  };
  let mut from_chat_id: i64 = row.get("tg_chat_id");
  let mut msg_id: i64 = row.get("tg_msg_id");

  match tg.forward_message(from_chat_id, chat_id, msg_id).await {
    Ok(_) => {
      return Ok(ShareResult { message: "Сообщение переслано.".into() });
    }
    Err(_) => {}
  }

  {
    let storage_chat_id = ensure_storage_chat_id(&state).await.map_err(map_err)?;
    if let Ok(Some((found_chat_id, found_msg_id))) =
      files::find_file_message(tg.as_ref(), from_chat_id, storage_chat_id, &file_id).await
    {
      if found_chat_id != from_chat_id || found_msg_id != msg_id {
        from_chat_id = found_chat_id;
        msg_id = found_msg_id;
        sqlx::query("UPDATE files SET tg_chat_id = ?, tg_msg_id = ?, is_broken = 0 WHERE id = ?")
          .bind(from_chat_id)
          .bind(msg_id)
          .bind(&file_id)
          .execute(db.pool())
          .await
          .map_err(|e| map_err(e.into()))?;
      }
    }
  }

  tg.forward_message(from_chat_id, chat_id, msg_id)
    .await
    .map_err(|e| e.to_string())?;

  Ok(ShareResult { message: "Сообщение переслано.".into() })
}

#[tauri::command]
pub async fn tg_test_message(state: State<'_, AppState>) -> Result<(), String> {
  info!(event = "tg_test_message", "Проверка связи с Telegram");
  let tg = state.telegram().map_err(map_err)?;
  let chat_id = ensure_storage_chat_id(&state).await.map_err(map_err)?;
  let ts = Utc::now().to_rfc3339();
  let text = format!("CloudTG: тестовое сообщение ({ts})");
  tg.send_text_message(chat_id, text).await.map_err(|e| e.to_string())?;
  Ok(())
}

#[tauri::command]
pub async fn tg_create_channel(state: State<'_, AppState>) -> Result<(), String> {
  info!(event = "tg_create_channel", "Создание нового канала хранения");
  let db = state.db().map_err(map_err)?;
  let pool = db.pool();
  let old_id = sync::get_sync(pool, "storage_chat_id")
    .await
    .map_err(map_err)?
    .and_then(|v| v.parse::<i64>().ok())
    .filter(|id| *id != 777);

  let tg = state.telegram().map_err(map_err)?;
  let new_id = tg.storage_create_channel().await.map_err(|e| e.to_string())?;
  sync::set_sync(pool, "storage_chat_id", &new_id.to_string()).await.map_err(map_err)?;

  if let Err(e) = reseed_storage_channel(pool, tg.as_ref(), old_id, new_id).await {
    tracing::error!(event = "storage_channel_reseed_failed", error = %e, "Не удалось пересоздать содержимое канала");
    return Err(format!("Не удалось перенести данные: {e}"));
  }

  if let Some(old) = old_id.filter(|id| *id != new_id) {
    if let Err(e) = tg.storage_delete_channel(old).await {
      tracing::warn!(event = "storage_channel_delete_failed", chat_id = old, error = %e, "Не удалось удалить старый канал");
    }
  }

  Ok(())
}

#[tauri::command]
pub async fn tg_sync_storage(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
  let res: Result<(), String> = async {
    info!(event = "storage_sync_start", "Синхронизация данных из Telegram");
    emit_sync(&app, "start", "Ищу сообщения в канале хранения", 0, None);

    let db = state.db().map_err(map_err)?;
    let pool = db.pool();
    let existing_dirs: i64 = sqlx::query("SELECT COUNT(1) as cnt FROM directories")
      .fetch_one(pool)
      .await
      .map_err(|e| e.to_string())?
      .get::<i64,_>("cnt");
    let existing_files: i64 = sqlx::query("SELECT COUNT(1) as cnt FROM files")
      .fetch_one(pool)
      .await
      .map_err(|e| e.to_string())?
      .get::<i64,_>("cnt");
    if existing_dirs > 0 || existing_files > 0 {
      info!(
        event = "storage_sync_incremental",
        dirs = existing_dirs,
        files = existing_files,
        "Локальные данные уже есть, проверяю новые сообщения"
      );
      emit_sync(&app, "progress", "Проверяю новые сообщения канала", 0, None);
    }

    let tg = state.telegram().map_err(map_err)?;
    let chat_id = ensure_storage_chat_id(&state).await.map_err(map_err)?;

    let mut from_message_id: i64 = 0;
    let mut processed: i64 = 0;
    let total: Option<i64> = None;
    let mut dir_count: i64 = 0;
    let mut file_count: i64 = 0;
    let mut imported_count: i64 = 0;
    let mut failed_count: i64 = 0;
    let mut unassigned_dir: Option<(String, String)> = None;

    let last_seen: i64 = sync::get_sync(pool, "storage_last_message_id")
      .await
      .map_err(|e| e.to_string())?
      .and_then(|v| v.parse::<i64>().ok())
      .unwrap_or(0);
    let mut newest_seen: Option<i64> = None;
    let mut stop = false;

    loop {
      let batch = tg
        .chat_history(chat_id, from_message_id, 100)
        .await
        .map_err(|e| e.to_string())?;

      if batch.messages.is_empty() {
        break;
      }

      for msg in batch.messages {
        if last_seen > 0 && msg.id <= last_seen {
          stop = true;
          break;
        }
        processed += 1;
        if newest_seen.is_none() {
          newest_seen = Some(msg.id);
        }
        let outcome = indexer::index_storage_message(pool, tg.as_ref(), chat_id, &msg, &mut unassigned_dir)
          .await
          .map_err(map_err)?;
        if outcome.dir {
          dir_count += 1;
        }
        if outcome.file {
          file_count += 1;
        }
        if outcome.imported {
          imported_count += 1;
        }
        if outcome.failed {
          failed_count += 1;
        }
      }

      emit_sync(&app, "progress", "Читаю сообщения канала", processed, total);
      info!(
        event = "storage_sync_batch",
        processed = processed,
        dirs = dir_count,
        files = file_count,
        imported = imported_count,
        failed = failed_count,
        next_from_message_id = batch.next_from_message_id,
        "Обработан пакет сообщений"
      );

      if stop || batch.next_from_message_id == 0 || batch.next_from_message_id == from_message_id {
        break;
      }
      from_message_id = batch.next_from_message_id;
    }

    if let Some(latest) = newest_seen {
      sync::set_sync(pool, "storage_last_message_id", &latest.to_string()).await.map_err(map_err)?;
    }

    sync::set_sync(pool, "storage_sync_done", &Utc::now().to_rfc3339()).await.map_err(map_err)?;
    emit_sync(&app, "success", "Синхронизация завершена", processed, total);
    info!(
      event = "storage_sync_done",
      processed = processed,
      dirs = dir_count,
      files = file_count,
      imported = imported_count,
      failed = failed_count,
      "Синхронизация завершена"
    );

    Ok(())
  }.await;

  if let Err(err) = res.as_ref() {
    emit_sync(&app, "error", "Синхронизация не удалась", 0, None);
    tracing::error!(event = "storage_sync_error", error = err, "Ошибка синхронизации");
  }

  res
}

#[tauri::command]
pub async fn tg_reconcile_recent(
  app: AppHandle,
  state: State<'_, AppState>,
  limit: Option<i64>,
  force: Option<bool>
) -> Result<TgReconcileResult, String> {
  let res: Result<TgReconcileResult, String> = async {
    let limit = limit.unwrap_or(100).max(1);
    let db = state.db().map_err(map_err)?;
    let force = force.unwrap_or(false);

    let sync_done = sync::get_sync(db.pool(), "storage_sync_done").await.map_err(map_err)?;
    if sync_done.is_none() && !force {
      return Err(format!(
        "{RECONCILE_SYNC_REQUIRED}: Сначала запусти импорт из канала хранения или подтверди запуск без него."
      ));
    }

    emit_sync(&app, "start", &format!("Реконсайл последних {limit} сообщений"), 0, Some(limit));

    let tg = state.telegram().map_err(map_err)?;
    let chat_id = ensure_storage_chat_id(&state).await.map_err(map_err)?;

    let outcome = reconcile::reconcile_recent(db.pool(), tg.as_ref(), chat_id, limit)
      .await
      .map_err(map_err)?;

    let marked = outcome.marked_dirs + outcome.marked_files;
    let cleared = outcome.cleared_dirs + outcome.cleared_files;
    let message = format!(
      "Готово: просмотрено {}, битых отмечено {}, восстановлено {}, импортировано {}.",
      outcome.scanned, marked, cleared, outcome.imported
    );

    emit_sync(&app, "success", "Реконсайл завершен", outcome.scanned, Some(limit));
    if outcome.scanned > 0 && (marked > 0 || cleared > 0 || outcome.imported > 0) {
      let _ = app.emit("tree_updated", ());
    }

    Ok(TgReconcileResult {
      message,
      scanned: outcome.scanned,
      marked,
      cleared,
      imported: outcome.imported
    })
  }
  .await;

  if let Err(err) = res.as_ref() {
    emit_sync(&app, "error", "Реконсайл не удался", 0, None);
    tracing::error!(event = "storage_reconcile_error", error = err, "Ошибка реконсайла");
  }

  res
}

#[tauri::command]
pub async fn backup_create(state: State<'_, AppState>) -> Result<BackupResult, String> {
  let db = state.db().map_err(map_err)?;
  let tg = state.telegram().map_err(map_err)?;
  let paths = state.paths().map_err(map_err)?;
  let chat_id = ensure_backup_chat_id(&state).await.map_err(map_err)?;

  let snapshot = backup::create_backup_snapshot(db.pool(), &paths).await.map_err(map_err)?;
  let caption = backup::build_backup_caption(env!("CARGO_PKG_VERSION"));
  let res = tg.send_file(chat_id, snapshot.clone(), caption).await.map_err(|e| e.to_string())?;
  let _ = std::fs::remove_file(&snapshot);

  info!(event = "backup_created", chat_id = res.chat_id, message_id = res.message_id, "Бэкап отправлен в канал");
  Ok(BackupResult { message: "Бэкап создан и отправлен в канал CloudTG Backups.".into() })
}

#[tauri::command]
pub async fn backup_restore(state: State<'_, AppState>) -> Result<BackupResult, String> {
  let tg = state.telegram().map_err(map_err)?;
  let paths = state.paths().map_err(map_err)?;
  let backup_chat_id = ensure_backup_chat_id(&state).await.map_err(map_err)?;
  let storage_chat_id = ensure_storage_chat_id(&state).await.map_err(map_err)?;

  let backup_msg = tg
    .search_chat_messages(backup_chat_id, backup::BACKUP_TAG.to_string(), 0, 1)
    .await
    .map_err(|e| e.to_string())?
    .messages
    .into_iter()
    .next();

  let latest_storage_date = tg
    .chat_history(storage_chat_id, 0, 1)
    .await
    .map_err(|e| e.to_string())?
    .messages
    .first()
    .map(|m| m.date)
    .unwrap_or(0);

  let pending_path = paths.pending_restore_path();
  if pending_path.exists() {
    let _ = std::fs::remove_file(&pending_path);
  }

  if let Some(msg) = backup_msg {
    if latest_storage_date == 0 || msg.date >= latest_storage_date {
      tg.download_message_file(backup_chat_id, msg.id, pending_path)
        .await
        .map_err(|e| e.to_string())?;
      return Ok(BackupResult {
        message: "Бэкап найден. Перезапусти приложение, чтобы применить восстановление.".into()
      });
    }
  }

  let tdlib_path = match state.db() {
    Ok(db) => settings::get_tdlib_path(db.pool()).await.ok().flatten(),
    Err(_) => None
  };
  let tdlib_effective = resolve_tdlib_path_effective(&paths, tdlib_path.as_deref())
    .map(|p| p.to_string_lossy().to_string());

  backup::rebuild_storage_to_path(
    &pending_path,
    tg.as_ref(),
    storage_chat_id,
    tdlib_effective.as_deref()
  )
    .await
    .map_err(map_err)?;
  Ok(BackupResult {
    message: "Актуальный бэкап не найден. Подготовлена новая база из канала хранения. Перезапусти приложение.".into()
  })
}

#[tauri::command]
pub async fn backup_open_channel(state: State<'_, AppState>) -> Result<BackupResult, String> {
  let tg = state.telegram().map_err(map_err)?;
  let backup_chat_id = ensure_backup_chat_id(&state).await.map_err(map_err)?;

  let backup_msg = tg
    .search_chat_messages(backup_chat_id, backup::BACKUP_TAG.to_string(), 0, 1)
    .await
    .map_err(|e| e.to_string())?
    .messages
    .into_iter()
    .next();

  let msg_id = if let Some(msg) = backup_msg {
    Some(msg.id)
  } else {
    tg.chat_history(backup_chat_id, 0, 1)
      .await
      .map_err(|e| e.to_string())?
      .messages
      .first()
      .map(|m| m.id)
  };

  let Some(msg_id) = msg_id else {
    return Ok(BackupResult {
      message: format!("Канал бэкапов пуст. chat_id={backup_chat_id}")
    });
  };

  let url = files::build_message_link(backup_chat_id, msg_id).map_err(map_err)?;
  open_url_in_os(&url).map_err(map_err)?;
  Ok(BackupResult { message: format!("Открываю канал: {url}") })
}

#[tauri::command]
pub async fn settings_get_tg(state: State<'_, AppState>) -> Result<TgSettingsView, String> {
  info!(event = "settings_get_tg", "Чтение настроек Telegram");
  let db = state.db().map_err(map_err)?;
  let mut tdlib_path = settings::get_tdlib_path(db.pool()).await.map_err(map_err)?;
  let paths = state.paths().map_err(map_err)?;
  if let Some(p) = resolve_tdlib_path_effective(&paths, tdlib_path.as_deref()) {
    tdlib_path = Some(p.to_string_lossy().to_string());
  }
  let runtime = state.tg_credentials().map(|(creds, _)| creds);
  let (_, status) = secrets::resolve_credentials(&paths, runtime.as_ref());

  Ok(TgSettingsView {
    tdlib_path,
    credentials: TgCredentialsView {
      available: status.available,
      source: status.source.map(|s| s.as_str().to_string()),
      keychain_available: status.keychain_available,
      encrypted_present: status.encrypted_present,
      locked: status.locked
    }
  })
}

#[tauri::command]
pub async fn settings_set_tg(state: State<'_, AppState>, input: TgSettingsInput) -> Result<TgSettingsSaveResult, String> {
  info!(
    event = "settings_set_tg",
    api_id = input.api_id.unwrap_or(0),
    api_hash_len = input.api_hash.as_ref().map(|v| v.len()).unwrap_or(0),
    tdlib_path_present = input.tdlib_path.as_ref().map(|p| !p.is_empty()).unwrap_or(false),
    remember = input.remember.unwrap_or(true),
    "Сохранение настроек Telegram"
  );

  let db = state.db().map_err(map_err)?;
  if let Some(p) = input.tdlib_path.as_ref().map(|p| p.trim().to_string()).filter(|p| !p.is_empty()) {
    let path = std::path::Path::new(&p);
    if !path.exists() {
      return Err("Указанный путь к TDLib не существует".into());
    }
    if !path.is_file() {
      return Err("Указанный путь к TDLib должен указывать на файл библиотеки".into());
    }
  }

  settings::set_tdlib_path(db.pool(), input.tdlib_path.clone()).await.map_err(map_err)?;

  let paths = state.paths().map_err(map_err)?;
  let remember = input.remember.unwrap_or(true);
  let storage_mode = input.storage_mode.as_deref().unwrap_or("keychain");
  let mut storage: Option<String> = None;

  let configured_creds = if let (Some(id), Some(hash)) = (input.api_id, input.api_hash.clone()) {
    let creds = secrets::normalize_credentials(id, hash).map_err(map_err)?;
    if !remember {
      state.set_tg_credentials(creds.clone(), CredentialsSource::Runtime);
      let _ = secrets::keychain_clear();
      let _ = secrets::encrypted_clear(&paths);
      storage = Some(CredentialsSource::Runtime.as_str().to_string());
      Some(creds)
    } else {
      match storage_mode {
        "encrypted" => {
          let password = input.password.clone().unwrap_or_default();
          if password.trim().is_empty() {
            return Err("Нужен пароль для шифрования.".into());
          }
          secrets::encrypted_save(&paths, &creds, &password).map_err(map_err)?;
          let _ = secrets::keychain_clear();
          state.set_tg_credentials(creds.clone(), CredentialsSource::EncryptedFile);
          storage = Some(CredentialsSource::EncryptedFile.as_str().to_string());
        }
        "keychain" | "auto" => {
          match secrets::keychain_set(&creds) {
            Ok(_) => {
              let _ = secrets::encrypted_clear(&paths);
              state.set_tg_credentials(creds.clone(), CredentialsSource::Keychain);
              storage = Some(CredentialsSource::Keychain.as_str().to_string());
            }
            Err(_) => {
              let password = input.password.clone().unwrap_or_default();
              if password.trim().is_empty() {
                return Err("Системное хранилище недоступно. Укажи пароль для шифрования.".into());
              }
              secrets::encrypted_save(&paths, &creds, &password).map_err(map_err)?;
              let _ = secrets::keychain_clear();
              state.set_tg_credentials(creds.clone(), CredentialsSource::EncryptedFile);
              storage = Some(CredentialsSource::EncryptedFile.as_str().to_string());
            }
          }
        }
        "runtime" => {
          state.set_tg_credentials(creds.clone(), CredentialsSource::Runtime);
          let _ = secrets::keychain_clear();
          let _ = secrets::encrypted_clear(&paths);
          storage = Some(CredentialsSource::Runtime.as_str().to_string());
        }
        _ => {
          return Err("Некорректный способ хранения ключей".into());
        }
      }
      Some(creds)
    }
  } else {
    let runtime = state.tg_credentials().map(|(creds, _)| creds);
    let (existing, _) = secrets::resolve_credentials(&paths, runtime.as_ref());
    existing
  };

  if let Some(creds) = configured_creds {
    let tg = state.telegram().map_err(map_err)?;
    tg.configure(creds.api_id, creds.api_hash, input.tdlib_path).await.map_err(|e| e.to_string())?;
    state.set_auth_state(AuthState::Unknown);
  } else {
    state.set_auth_state(AuthState::WaitConfig);
  }

  info!(event = "settings_set_tg_done", storage = storage.as_deref().unwrap_or("none"), "Настройки Telegram сохранены");
  let message = match storage.as_deref() {
    Some("keychain") => "Ключи сохранены в системном хранилище.".to_string(),
    Some("encrypted") => "Ключи сохранены в зашифрованном файле.".to_string(),
    Some("runtime") => "Ключи действуют только в текущем запуске.".to_string(),
    _ => "Настройки сохранены.".to_string()
  };
  Ok(TgSettingsSaveResult { storage, message })
}

#[tauri::command]
pub async fn settings_unlock_tg(state: State<'_, AppState>, password: String) -> Result<(), String> {
  info!(event = "settings_unlock_tg", password_len = password.len(), "Разблокировка ключей");
  if password.trim().is_empty() {
    return Err("Нужен пароль для расшифровки".into());
  }
  let paths = state.paths().map_err(map_err)?;
  if !secrets::encrypted_exists(&paths) {
    return Err("Зашифрованные ключи не найдены".into());
  }
  let creds = secrets::encrypted_load(&paths, &password).map_err(map_err)?;
  state.set_tg_credentials(creds.clone(), CredentialsSource::EncryptedFile);

  let db = state.db().map_err(map_err)?;
  let tdlib_path = settings::get_tdlib_path(db.pool()).await.map_err(map_err)?;
  let tg = state.telegram().map_err(map_err)?;
  tg.configure(creds.api_id, creds.api_hash, tdlib_path).await.map_err(|e| e.to_string())?;
  state.set_auth_state(AuthState::Unknown);
  Ok(())
}

async fn reseed_storage_channel(
  pool: &sqlx::SqlitePool,
  tg: &dyn crate::telegram::TelegramService,
  old_chat_id: Option<i64>,
  new_chat_id: i64
) -> anyhow::Result<()> {
  info!(event = "storage_channel_reseed_start", old_chat_id = old_chat_id.unwrap_or(0), new_chat_id = new_chat_id, "Пересоздание содержимого канала");

  let now = Utc::now().timestamp();
  let dir_rows = sqlx::query("SELECT id, parent_id, name FROM directories ORDER BY name")
    .fetch_all(pool)
    .await?;

  for r in dir_rows {
    let id: String = r.get("id");
    let name: String = r.get("name");
    let raw_parent = r.try_get::<String,_>("parent_id").ok();
    let parent_id = raw_parent.filter(|p| !p.trim().is_empty() && p != "ROOT").unwrap_or_else(|| "ROOT".to_string());
    let msg = make_dir_message(&DirMeta { dir_id: id.clone(), parent_id, name });
    let uploaded = tg.send_dir_message(new_chat_id, msg).await?;
    sqlx::query("UPDATE directories SET tg_msg_id = ?, updated_at = ?, is_broken = 0 WHERE id = ?")
      .bind(uploaded.message_id)
      .bind(now)
      .bind(&id)
      .execute(pool)
      .await?;
  }

  let file_rows = sqlx::query("SELECT id, tg_chat_id, tg_msg_id FROM files ORDER BY tg_chat_id, tg_msg_id")
    .fetch_all(pool)
    .await?;

  let mut current_chat: Option<i64> = None;
  let mut batch: Vec<(String, i64)> = Vec::new();

  for r in file_rows {
    let file_id: String = r.get("id");
    let chat_id: i64 = r.get("tg_chat_id");
    let msg_id: i64 = r.get("tg_msg_id");
    if current_chat.is_none() {
      current_chat = Some(chat_id);
    }
    if current_chat != Some(chat_id) {
      if let Some(c) = current_chat {
        flush_file_batch(pool, tg, old_chat_id, new_chat_id, c, &mut batch).await?;
      }
      current_chat = Some(chat_id);
    }
    batch.push((file_id, msg_id));
  }
  if let Some(c) = current_chat {
    flush_file_batch(pool, tg, old_chat_id, new_chat_id, c, &mut batch).await?;
  }

  info!(event = "storage_channel_reseed_done", "Пересоздание содержимого канала завершено");
  Ok(())
}

async fn flush_file_batch(
  pool: &sqlx::SqlitePool,
  tg: &dyn crate::telegram::TelegramService,
  old_chat_id: Option<i64>,
  new_chat_id: i64,
  chat_id: i64,
  items: &mut Vec<(String, i64)>
) -> anyhow::Result<()> {
  if items.is_empty() {
    return Ok(());
  }
  if chat_id == new_chat_id {
    items.clear();
    return Ok(());
  }
  if let Some(old) = old_chat_id {
    if chat_id != old {
      tracing::warn!(event = "storage_channel_reseed_skip", chat_id = chat_id, "Файлы относятся к другому каналу, пропускаю");
      items.clear();
      return Ok(());
    }
  }

  let mut start = 0;
  while start < items.len() {
    let end = (start + 100).min(items.len());
    let chunk = &items[start..end];
    let ids: Vec<i64> = chunk.iter().map(|(_, msg_id)| *msg_id).collect();
    let copied = tg.copy_messages(chat_id, new_chat_id, ids).await?;
    if copied.len() != chunk.len() {
      tracing::warn!(
        event = "storage_channel_reseed_copy_mismatch",
        expected = chunk.len(),
        got = copied.len(),
        "TDLib вернул неожиданное число сообщений при копировании"
      );
    }
    for (idx, result) in copied.into_iter().enumerate().take(chunk.len()) {
      if let Some(new_id) = result {
        let file_id = &chunk[idx].0;
        sqlx::query("UPDATE files SET tg_chat_id = ?, tg_msg_id = ?, is_broken = 0 WHERE id = ?")
          .bind(new_chat_id)
          .bind(new_id)
          .bind(file_id)
          .execute(pool)
          .await?;
      } else {
        let file_id = &chunk[idx].0;
        tracing::warn!(
          event = "storage_channel_reseed_file_failed",
          old_chat_id = chat_id,
          file_id = file_id,
          "Не удалось скопировать файл в новый канал"
        );
      }
    }
    start = end;
  }

  items.clear();
  Ok(())
}

fn mask_phone(phone: &str) -> String {
  let p = phone.trim();
  if p.len() <= 4 {
    return "***".to_string();
  }
  let tail = &p[p.len() - 4..];
  format!("***{tail}")
}

fn resolve_tdlib_path_effective(paths: &Paths, configured: Option<&str>) -> Option<PathBuf> {
  if let Some(p) = configured {
    let path = PathBuf::from(p);
    if path.exists() {
      return Some(path);
    }
  }

  if let Ok(p) = std::env::var("CLOUDTG_TDLIB_PATH") {
    let path = PathBuf::from(p);
    if path.exists() {
      return Some(path);
    }
  }

  let mut candidates = tdlib_platform_candidates(&paths.base_dir);
  if let Some(resource_dir) = paths.resource_dir.as_ref() {
    candidates.extend(tdlib_resource_candidates(resource_dir));
  }
  for c in candidates {
    if c.exists() {
      return Some(c);
    }
  }

  let repo_dir = tdlib_reserved_dir(paths).join("td");
  if repo_dir.exists() {
    if let Some(p) = find_tdjson_lib(&repo_dir) {
      return Some(p);
    }
  }

  None
}

fn tdlib_platform_candidates(base: &Path) -> Vec<PathBuf> {
  #[cfg(target_os = "windows")]
  let names = ["tdjson.dll"];

  #[cfg(target_os = "macos")]
  let names = ["libtdjson.dylib"];

  #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
  let names = ["libtdjson.so", "libtdjson.so.1"];

  names.iter().map(|name| base.join(name)).collect()
}

fn tdlib_resource_candidates(resource_dir: &Path) -> Vec<PathBuf> {
  let os = std::env::consts::OS;
  let arch = std::env::consts::ARCH;
  let mut bases = Vec::new();
  bases.push(resource_dir.join("tdlib").join(format!("{os}-{arch}")));
  bases.push(resource_dir.join("tdlib").join(os));
  bases.push(resource_dir.join("tdlib"));
  bases.push(resource_dir.to_path_buf());

  let mut out = Vec::new();
  for base in bases {
    out.extend(tdlib_platform_candidates(&base));
  }
  out
}

fn tdlib_reserved_dir(paths: &Paths) -> PathBuf {
  let base = paths.base_dir.join("third_party").join("tdlib");
  if base.exists() {
    return base;
  }
  if let Ok(cwd) = std::env::current_dir() {
    let alt = cwd.join("third_party").join("tdlib");
    if alt.exists() {
      return alt;
    }
  }
  base
}

fn find_tdjson_lib(root: &Path) -> Option<PathBuf> {
  let mut stack = vec![root.to_path_buf()];
  let names = [
    "libtdjson.so",
    "libtdjson.so.1",
    "libtdjson.dylib",
    "tdjson.dll"
  ];

  while let Some(dir) = stack.pop() {
    let entries = match std::fs::read_dir(&dir) {
      Ok(v) => v,
      Err(_) => continue
    };
    for e in entries.flatten() {
      let path = e.path();
      if path.is_dir() {
        stack.push(path);
      } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        if names.iter().any(|n| n == &name) {
          return Some(path);
        }
      }
    }
  }
  None
}
