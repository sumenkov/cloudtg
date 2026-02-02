use tauri::{Emitter, State, AppHandle};
use std::path::{Path, PathBuf};
use sqlx::Row;
use chrono::Utc;
use serde::Deserialize;

use crate::state::{AppState, AuthState};
use crate::app::{dirs, sync, files};
use crate::settings;
use crate::secrets::{self, CredentialsSource};
use crate::paths::Paths;
use crate::fsmeta::{DirMeta, make_dir_message, parse_dir_message, parse_file_caption, FileMeta};
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

async fn download_file_path(state: &AppState, file_id: &str) -> anyhow::Result<PathBuf> {
  let db = state.db()?;
  let tg = state.telegram()?;
  let paths = state.paths()?;
  let storage_chat_id = ensure_storage_chat_id(state).await?;
  files::download_file(db.pool(), tg.as_ref(), &paths, storage_chat_id, file_id).await
}

fn open_file_in_os(path: &Path) -> anyhow::Result<()> {
  let path_str = path.to_string_lossy().to_string();
  #[cfg(target_os = "windows")]
  {
    std::process::Command::new("cmd")
      .args(["/C", "start", "", &path_str])
      .spawn()?;
    return Ok(());
  }
  #[cfg(target_os = "macos")]
  {
    std::process::Command::new("open")
      .arg(&path_str)
      .spawn()?;
    return Ok(());
  }
  #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
  {
    std::process::Command::new("xdg-open")
      .arg(&path_str)
      .spawn()?;
    return Ok(());
  }
}

fn open_folder_for_file(path: &Path) -> anyhow::Result<()> {
  let path_str = path.to_string_lossy().to_string();
  #[cfg(target_os = "windows")]
  {
    std::process::Command::new("explorer")
      .arg(format!("/select,{path_str}"))
      .spawn()?;
    return Ok(());
  }
  #[cfg(target_os = "macos")]
  {
    std::process::Command::new("open")
      .arg("-R")
      .arg(&path_str)
      .spawn()?;
    return Ok(());
  }
  #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
  {
    let folder = path.parent().unwrap_or_else(|| Path::new("."));
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
pub async fn dir_list_tree(state: State<'_, AppState>) -> Result<crate::app::models::DirNode, String> {
  let db = state.db().map_err(map_err)?;
  dirs::list_tree(db.pool()).await.map_err(map_err)
}

#[tauri::command]
pub async fn file_list(state: State<'_, AppState>, dir_id: String) -> Result<Vec<files::FileItem>, String> {
  let db = state.db().map_err(map_err)?;
  files::list_files(db.pool(), &dir_id).await.map_err(map_err)
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
  files::delete_file(db.pool(), tg.as_ref(), &file_id).await.map_err(map_err)?;
  Ok(())
}

#[tauri::command]
pub async fn file_delete_many(state: State<'_, AppState>, file_ids: Vec<String>) -> Result<(), String> {
  info!(event = "file_delete_many", count = file_ids.len(), "Удаление нескольких файлов");
  let db = state.db().map_err(map_err)?;
  let tg = state.telegram().map_err(map_err)?;
  files::delete_files(db.pool(), tg.as_ref(), &file_ids).await.map_err(map_err)?;
  Ok(())
}

#[tauri::command]
pub async fn file_download(state: State<'_, AppState>, file_id: String) -> Result<String, String> {
  info!(event = "file_download", file_id = file_id.as_str(), "Скачивание файла");
  let path = download_file_path(&state, &file_id).await.map_err(map_err)?;
  Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn file_open(state: State<'_, AppState>, file_id: String) -> Result<(), String> {
  let path = download_file_path(&state, &file_id).await.map_err(map_err)?;
  open_file_in_os(&path).map_err(map_err)?;
  Ok(())
}

#[tauri::command]
pub async fn file_open_folder(state: State<'_, AppState>, file_id: String) -> Result<(), String> {
  let path = download_file_path(&state, &file_id).await.map_err(map_err)?;
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
        sqlx::query("UPDATE files SET tg_chat_id = ?, tg_msg_id = ? WHERE id = ?")
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
    emit_sync(&app, "start", "Ищу сообщения CloudTG в канале", 0, None);

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
        event = "storage_sync_skip",
        dirs = existing_dirs,
        files = existing_files,
        "Локальные данные уже есть, синхронизация не требуется"
      );
      emit_sync(&app, "skip", "Локальные данные уже есть, синхронизация не требуется", 0, None);
      return Ok(());
    }

    let tg = state.telegram().map_err(map_err)?;
    let chat_id = ensure_storage_chat_id(&state).await.map_err(map_err)?;

    let mut from_message_id: i64 = 0;
    let mut processed: i64 = 0;
    let mut total: Option<i64> = None;
    let mut dir_count: i64 = 0;
    let mut file_count: i64 = 0;

    loop {
      let batch = tg
        .search_storage_messages(chat_id, from_message_id, 100)
        .await
        .map_err(|e| e.to_string())?;

      if total.is_none() {
        total = batch.total_count;
        if let Some(t) = total {
          info!(event = "storage_sync_total", total = t, "Оценка количества сообщений");
        }
      }

      if batch.messages.is_empty() {
        break;
      }

      for msg in batch.messages {
        processed += 1;
        if let Some(text) = msg.text.as_deref() {
          if let Ok(meta) = parse_dir_message(text) {
            upsert_dir(pool, &meta, msg.id, msg.date).await.map_err(map_err)?;
            dir_count += 1;
            continue;
          }
        }

        if let Some(caption) = msg.caption.as_deref() {
          if let Ok(meta) = parse_file_caption(caption) {
            upsert_file(pool, &meta, chat_id, msg.id, msg.date, msg.file_size.unwrap_or(0)).await.map_err(map_err)?;
            file_count += 1;
            continue;
          }
        }
      }

      emit_sync(&app, "progress", "Читаю сообщения канала", processed, total);
      info!(
        event = "storage_sync_batch",
        processed = processed,
        dirs = dir_count,
        files = file_count,
        next_from_message_id = batch.next_from_message_id,
        "Обработан пакет сообщений"
      );

      if batch.next_from_message_id == 0 {
        break;
      }
      from_message_id = batch.next_from_message_id;
    }

    sync::set_sync(pool, "storage_sync_done", &Utc::now().to_rfc3339()).await.map_err(map_err)?;
    emit_sync(&app, "success", "Синхронизация завершена", processed, total);
    info!(
      event = "storage_sync_done",
      processed = processed,
      dirs = dir_count,
      files = file_count,
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
    sqlx::query("UPDATE directories SET tg_msg_id = ?, updated_at = ? WHERE id = ?")
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
        sqlx::query("UPDATE files SET tg_chat_id = ?, tg_msg_id = ? WHERE id = ?")
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

async fn upsert_dir(pool: &sqlx::SqlitePool, meta: &DirMeta, msg_id: i64, date: i64) -> anyhow::Result<()> {
  let parent_id = if meta.parent_id == "ROOT" || meta.parent_id.trim().is_empty() {
    None
  } else {
    Some(meta.parent_id.as_str())
  };
  if let Some(pid) = parent_id {
    ensure_dir_placeholder(pool, pid, date).await?;
  }
  sqlx::query(
    "INSERT INTO directories(id, parent_id, name, tg_msg_id, updated_at) VALUES(?, ?, ?, ?, ?)
     ON CONFLICT(id) DO UPDATE SET parent_id=excluded.parent_id, name=excluded.name, tg_msg_id=excluded.tg_msg_id, updated_at=excluded.updated_at"
  )
    .bind(&meta.dir_id)
    .bind(parent_id)
    .bind(&meta.name)
    .bind(msg_id)
    .bind(date)
    .execute(pool)
    .await?;
  Ok(())
}

async fn upsert_file(
  pool: &sqlx::SqlitePool,
  meta: &FileMeta,
  chat_id: i64,
  msg_id: i64,
  date: i64,
  size: i64
) -> anyhow::Result<()> {
  ensure_dir_placeholder(pool, &meta.dir_id, date).await?;

  sqlx::query(
    "INSERT INTO files(id, dir_id, name, size, hash, tg_chat_id, tg_msg_id, created_at)
     VALUES(?, ?, ?, ?, ?, ?, ?, ?)
     ON CONFLICT(id) DO UPDATE SET dir_id=excluded.dir_id, name=excluded.name, size=excluded.size, hash=excluded.hash, tg_chat_id=excluded.tg_chat_id, tg_msg_id=excluded.tg_msg_id, created_at=excluded.created_at"
  )
    .bind(&meta.file_id)
    .bind(&meta.dir_id)
    .bind(&meta.name)
    .bind(size)
    .bind(&meta.hash_short)
    .bind(chat_id)
    .bind(msg_id)
    .bind(date)
    .execute(pool)
    .await?;
  Ok(())
}

async fn ensure_dir_placeholder(pool: &sqlx::SqlitePool, dir_id: &str, date: i64) -> anyhow::Result<()> {
  if dir_id.trim().is_empty() {
    return Ok(());
  }
  let placeholder = "Неизвестная папка";
  let inserted = sqlx::query(
    "INSERT INTO directories(id, parent_id, name, tg_msg_id, updated_at)
     VALUES(?, NULL, ?, NULL, ?)
     ON CONFLICT(id) DO NOTHING"
  )
    .bind(dir_id)
    .bind(placeholder)
    .bind(date)
    .execute(pool)
    .await?;
  if inserted.rows_affected() > 0 {
    tracing::debug!(
      event = "storage_sync_dir_placeholder",
      dir_id = dir_id,
      "Добавлена заглушка директории"
    );
  }
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
