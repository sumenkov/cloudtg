use tauri::{Emitter, State, AppHandle};
use std::path::{Path, PathBuf};

use crate::state::{AppState, AuthState};
use crate::app::{dirs, sync};
use crate::settings;
use crate::paths::Paths;
use tracing::info;

#[derive(serde::Serialize)]
pub struct AuthStatus { pub state: String }

fn map_err(e: anyhow::Error) -> String { format!("{e:#}") }

async fn ensure_storage_chat_id(state: &AppState) -> anyhow::Result<i64> {
  let db = state.db()?;
  let pool = db.pool();

  if let Some(v) = sync::get_sync(pool, "storage_chat_id").await? {
    if let Ok(id) = v.parse::<i64>() {
      if id == 777 {
        info!(event = "storage_chat_id_invalid", value = v, "Обнаружен mock chat_id, пересоздаю");
      } else {
        info!(event = "storage_chat_id_cached", chat_id = id, "Использую сохраненный storage_chat_id");
        return Ok(id);
      }
    } else {
      info!(event = "storage_chat_id_invalid", value = v, "Некорректный storage_chat_id, пересоздаю");
    }
  }

  let tg = state.telegram()?;
  let chat_id = tg.storage_get_or_create_channel().await?;
  sync::set_sync(pool, "storage_chat_id", &chat_id.to_string()).await?;
  info!(event = "storage_chat_id_saved", chat_id = chat_id, "storage_chat_id сохранен");
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
pub async fn dir_list_tree(state: State<'_, AppState>) -> Result<crate::app::models::DirNode, String> {
  let db = state.db().map_err(map_err)?;
  dirs::list_tree(db.pool()).await.map_err(map_err)
}

#[tauri::command]
pub async fn settings_get_tg(state: State<'_, AppState>) -> Result<settings::TgSettingsView, String> {
  info!(event = "settings_get_tg", "Чтение настроек Telegram");
  let db = state.db().map_err(map_err)?;
  let mut view = settings::get_tg_settings_view(db.pool()).await.map_err(map_err)?;
  if let Ok(paths) = state.paths() {
    if let Some(p) = resolve_tdlib_path_effective(&paths, view.tdlib_path.as_deref()) {
      view.tdlib_path = Some(p.to_string_lossy().to_string());
    }
  }
  Ok(view)
}

#[tauri::command]
pub async fn settings_set_tg(
  state: State<'_, AppState>,
  api_id: i32,
  api_hash: String,
  tdlib_path: Option<String>
) -> Result<(), String> {
  info!(
    event = "settings_set_tg",
    api_id = api_id,
    api_hash_len = api_hash.len(),
    tdlib_path_present = tdlib_path.as_ref().map(|p| !p.is_empty()).unwrap_or(false),
    "Сохранение настроек Telegram"
  );
  if api_id <= 0 {
    return Err("API_ID должен быть положительным числом".into());
  }
  if api_hash.trim().is_empty() {
    return Err("API_HASH не может быть пустым".into());
  }

  let db = state.db().map_err(map_err)?;
  if let Some(p) = tdlib_path.as_ref().map(|p| p.trim().to_string()).filter(|p| !p.is_empty()) {
    let path = std::path::Path::new(&p);
    if !path.exists() {
      return Err("Указанный путь к TDLib не существует".into());
    }
    if !path.is_file() {
      return Err("Указанный путь к TDLib должен указывать на файл библиотеки".into());
    }
  }

  settings::set_tg_settings(db.pool(), api_id, api_hash.clone(), tdlib_path.clone()).await.map_err(map_err)?;

  let tg = state.telegram().map_err(map_err)?;
  tg.configure(api_id, api_hash, tdlib_path).await.map_err(|e| e.to_string())?;
  state.set_auth_state(AuthState::Unknown);
  info!(event = "settings_set_tg_done", "Настройки Telegram сохранены");
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
