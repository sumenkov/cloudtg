use std::{
  collections::HashMap,
  ffi::{CStr, CString},
  io::{BufRead, BufReader, Read, Write, Cursor},
  os::raw::{c_char, c_double, c_int, c_void},
  path::{Path, PathBuf},
  process::{Command, Stdio},
  sync::mpsc,
  time::Duration
};

use libloading::Library;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::{Emitter, Manager};
use tempfile::NamedTempFile;
use tokio::sync::oneshot;
use flate2::read::GzDecoder;
use tar::Archive;
use zip::ZipArchive;
use image::{DynamicImage, ImageFormat, RgbaImage};
use image::imageops::FilterType;
use chrono::Utc;
use parking_lot::Mutex;

use crate::paths::Paths;
use crate::state::{AppState, AuthState};
use crate::secrets::TgCredentials;
use crate::app::{indexer, sync};
use super::{ChatId, MessageId, TelegramService, TgError, UploadedMessage, HistoryMessage, SearchMessagesResult, ChatInfo};

#[derive(Clone)]
struct TdlibConfig {
  api_id: i32,
  api_hash: String,
  db_dir: PathBuf,
  files_dir: PathBuf
}

impl TdlibConfig {
  fn from_settings(
    paths: &Paths,
    api_id: i32,
    api_hash: String,
    session_name: &str
  ) -> anyhow::Result<Self> {
    let db_dir = paths.data_dir.join("tdlib").join(session_name);
    let files_dir = paths.cache_dir.join("tdlib_files").join(session_name);
    std::fs::create_dir_all(&db_dir)?;
    std::fs::create_dir_all(&files_dir)?;

    Ok(Self { api_id, api_hash, db_dir, files_dir })
  }
}

struct TdlibClient {
  _lib: Library,
  client: *mut c_void,
  send: unsafe extern "C" fn(*mut c_void, *const c_char),
  receive: unsafe extern "C" fn(*mut c_void, c_double) -> *const c_char,
  execute: unsafe extern "C" fn(*mut c_void, *const c_char) -> *const c_char,
  destroy: unsafe extern "C" fn(*mut c_void),
  set_log_verbosity: Option<unsafe extern "C" fn(c_int)>
}

// Безопасно, пока клиент используется только в одном потоке.
unsafe impl Send for TdlibClient {}

impl TdlibClient {
  fn load(path: &Path) -> anyhow::Result<Self> {
    let mut resolved = path.to_path_buf();
    if !resolved.is_absolute() {
      if let Ok(cwd) = std::env::current_dir() {
        resolved = cwd.join(&resolved);
      }
    }
    let resolved = std::fs::canonicalize(&resolved).unwrap_or(resolved);

    unsafe {
      #[cfg(target_os = "windows")]
      let lib: Library = {
        use libloading::os::windows::{
          Library as WinLibrary,
          LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
          LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
          LOAD_WITH_ALTERED_SEARCH_PATH
        };

        // When loading a DLL from an arbitrary folder, ensure the DLL directory is searched for
        // its dependencies. This is important for portable Windows builds.
        let flags = LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS;
        match WinLibrary::load_with_flags(&resolved, flags) {
          Ok(l) => l.into(),
          Err(e1) => match WinLibrary::load_with_flags(&resolved, LOAD_WITH_ALTERED_SEARCH_PATH) {
            Ok(l) => l.into(),
            Err(e2) => {
              return Err(anyhow::anyhow!(
                "Не удалось загрузить библиотеку TDLib по пути {}: {e1} (fallback: {e2}). Возможно, рядом отсутствуют зависимые DLL.",
                resolved.display()
              ));
            }
          }
        }
      };

      #[cfg(not(target_os = "windows"))]
      let lib = Library::new(&resolved)?;
      let create = *lib.get::<unsafe extern "C" fn() -> *mut c_void>(b"td_json_client_create")?;
      let send = *lib.get::<unsafe extern "C" fn(*mut c_void, *const c_char)>(b"td_json_client_send")?;
      let receive = *lib.get::<unsafe extern "C" fn(*mut c_void, c_double) -> *const c_char>(b"td_json_client_receive")?;
      let execute = *lib.get::<unsafe extern "C" fn(*mut c_void, *const c_char) -> *const c_char>(b"td_json_client_execute")?;
      let destroy = *lib.get::<unsafe extern "C" fn(*mut c_void)>(b"td_json_client_destroy")?;

      let set_log_verbosity = lib.get::<unsafe extern "C" fn(c_int)>(b"td_set_log_verbosity_level").ok().map(|f| *f);

      let client = create();
      if client.is_null() {
        return Err(anyhow::anyhow!("Не удалось создать TDLib клиента"));
      }

      Ok(Self {
        _lib: lib,
        client,
        send,
        receive,
        execute,
        destroy,
        set_log_verbosity
      })
    }
  }

  fn send(&self, query: &str) -> Result<(), TgError> {
    let c = CString::new(query).map_err(|_| TgError::Other("Некорректная строка для TDLib".into()))?;
    unsafe { (self.send)(self.client, c.as_ptr()); }
    Ok(())
  }

  fn receive(&self, timeout: f64) -> Option<String> {
    let ptr = unsafe { (self.receive)(self.client, timeout) };
    if ptr.is_null() { return None; }
    let s = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().to_string();
    Some(s)
  }

  #[allow(dead_code)]
  fn execute(&self, query: &str) -> Option<String> {
    let c = CString::new(query).ok()?;
    let ptr = unsafe { (self.execute)(self.client, c.as_ptr()) };
    if ptr.is_null() { return None; }
    Some(unsafe { CStr::from_ptr(ptr) }.to_string_lossy().to_string())
  }

  fn set_verbosity(&self, level: i32) {
    if let Some(f) = self.set_log_verbosity {
      unsafe { f(level); }
    }
  }

  fn destroy(self) {
    unsafe { (self.destroy)(self.client); }
  }
}

pub struct TdlibTelegram {
  tx: mpsc::Sender<TdlibCommand>,
  paths: Paths,
  send_waiters: SendWaiters,
  send_results: SendResults
}

enum TdlibCommand {
  Td(String),
  SetConfig { api_id: i32, api_hash: String, tdlib_path: Option<String> },
  Request { payload: Value, respond_to: oneshot::Sender<anyhow::Result<Value>> }
}

type PendingRequests = HashMap<u64, oneshot::Sender<anyhow::Result<Value>>>;
type SendWaiters = std::sync::Arc<Mutex<HashMap<i64, oneshot::Sender<anyhow::Result<i64>>>>>;
type SendResults = std::sync::Arc<Mutex<HashMap<i64, Result<i64, String>>>>;

const STORAGE_CHANNEL_TITLE: &str = "CloudTG";
const STORAGE_CHANNEL_TITLE_LEGACY: &str = "CloudVault";
const BACKUP_CHANNEL_TITLE: &str = "CloudTG Backups";
const TDLIB_MANIFEST_NAME: &str = "tdlib-manifest.json";

fn storage_channel_title() -> &'static str {
  STORAGE_CHANNEL_TITLE
}

fn storage_channel_title_legacy() -> &'static str {
  STORAGE_CHANNEL_TITLE_LEGACY
}

fn backup_channel_title() -> &'static str {
  BACKUP_CHANNEL_TITLE
}

fn tdlib_session_name() -> String {
  if let Ok(raw) = std::env::var("CLOUDTG_TDLIB_SESSION") {
    let name = normalize_session_name(&raw);
    if !name.is_empty() {
      return name;
    }
  }
  if cfg!(debug_assertions) {
    "cloudtg-dev".to_string()
  } else {
    "cloudtg".to_string()
  }
}

fn normalize_session_name(raw: &str) -> String {
  let trimmed = raw.trim();
  if trimmed.is_empty() {
    return String::new();
  }
  trimmed
    .chars()
    .map(|c| {
      if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
        c
      } else {
        '_'
      }
    })
    .collect()
}

fn app_icon_bytes() -> &'static [u8] {
  include_bytes!("../../icons/icon.png")
}

const TELEGRAM_ICON_SIZE: u32 = 512;
const TELEGRAM_ICON_SAFE_SCALE: f32 = 0.70;

fn build_telegram_icon(bytes: &[u8]) -> anyhow::Result<Vec<u8>> {
  let decoded = image::load_from_memory(bytes)?;
  let size = TELEGRAM_ICON_SIZE;
  let safe = ((size as f32) * TELEGRAM_ICON_SAFE_SCALE).round().max(1.0) as u32;
  let resized = decoded.resize(safe, safe, FilterType::Lanczos3);
  let mut canvas = RgbaImage::from_pixel(size, size, image::Rgba([0, 0, 0, 0]));
  let x = (size - resized.width()) / 2;
  let y = (size - resized.height()) / 2;
  image::imageops::overlay(&mut canvas, &resized, x.into(), y.into());
  let mut out = Vec::new();
  let mut cursor = Cursor::new(&mut out);
  DynamicImage::ImageRgba8(canvas).write_to(&mut cursor, ImageFormat::Png)?;
  Ok(out)
}

fn extract_text(content: &Value) -> Option<String> {
  let t = content.get("@type").and_then(|v| v.as_str()).unwrap_or("");
  if t == "messageText" {
    return content
      .get("text")
      .and_then(|v| v.get("text"))
      .and_then(|v| v.as_str())
      .map(|s| s.to_string());
  }
  None
}

fn extract_caption(content: &Value) -> Option<String> {
  content
    .get("caption")
    .and_then(|v| v.get("text"))
    .and_then(|v| v.as_str())
    .map(|s| s.to_string())
}

fn extract_file_size(content: &Value) -> Option<i64> {
  if let Some(size) = content.get("file_size").and_then(|v| v.as_i64()) {
    return Some(size);
  }

  let candidates = [
    "document",
    "video",
    "audio",
    "voice_note",
    "video_note",
    "animation",
    "sticker",
    "photo"
  ];

  for key in candidates {
    if let Some(obj) = content.get(key).and_then(|v| v.as_object()) {
      if let Some(size) = obj.get("file_size").and_then(|v| v.as_i64()) {
        return Some(size);
      }
      if let Some(file) = obj.get("file").and_then(|v| v.as_object()) {
        if let Some(size) = file.get("size").and_then(|v| v.as_i64()) {
          return Some(size);
        }
      }
      if let Some(doc) = obj.get("document").and_then(|v| v.as_object()) {
        if let Some(file) = doc.get("file").and_then(|v| v.as_object()) {
          if let Some(size) = file.get("size").and_then(|v| v.as_i64()) {
            return Some(size);
          }
        }
        if let Some(size) = doc.get("file_size").and_then(|v| v.as_i64()) {
          return Some(size);
        }
      }
    }
  }

  None
}

fn extract_file_name(content: &Value) -> Option<String> {
  if let Some(name) = content.get("file_name").and_then(|v| v.as_str()) {
    if !name.trim().is_empty() {
      return Some(name.to_string());
    }
  }

  let candidates = [
    "document",
    "video",
    "audio",
    "voice_note",
    "video_note",
    "animation",
    "sticker",
    "photo"
  ];

  for key in candidates {
    if let Some(obj) = content.get(key).and_then(|v| v.as_object()) {
      if let Some(name) = obj.get("file_name").and_then(|v| v.as_str()) {
        if !name.trim().is_empty() {
          return Some(name.to_string());
        }
      }
      if let Some(doc) = obj.get("document").and_then(|v| v.as_object()) {
        if let Some(name) = doc.get("file_name").and_then(|v| v.as_str()) {
          if !name.trim().is_empty() {
            return Some(name.to_string());
          }
        }
      }
    }
  }

  None
}

fn history_message_from_content(message_id: i64, date: i64, content: &Value) -> HistoryMessage {
  let (text, caption, file_size, file_name) = (
    extract_text(content),
    extract_caption(content),
    extract_file_size(content),
    extract_file_name(content)
  );
  HistoryMessage { id: message_id, date, text, caption, file_size, file_name }
}

fn history_message_from_object(message: &Value) -> Option<(ChatId, HistoryMessage)> {
  let chat_id = message.get("chat_id").and_then(|v| v.as_i64())?;
  let message_id = message.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
  if message_id == 0 {
    return None;
  }
  let date = message.get("date").and_then(|v| v.as_i64()).unwrap_or(0);
  let (text, caption, file_size, file_name) = if let Some(content) = message.get("content") {
    (extract_text(content), extract_caption(content), extract_file_size(content), extract_file_name(content))
  } else {
    (None, None, None, None)
  };
  Some((chat_id, HistoryMessage { id: message_id, date, text, caption, file_size, file_name }))
}

fn schedule_storage_index(app: &tauri::AppHandle, chat_id: i64, msg: HistoryMessage) {
  let app = app.clone();
  tauri::async_runtime::spawn(async move {
    let state = app.state::<AppState>();
    let db = match state.db() {
      Ok(db) => db,
      Err(e) => {
        tracing::debug!(event = "storage_index_skip", error = %e, "База данных еще не готова");
        return;
      }
    };
    let pool = db.pool();
    let storage_chat_id = match sync::get_sync(pool, "storage_chat_id").await {
      Ok(Some(v)) => v.parse::<i64>().ok(),
      Ok(None) => None,
      Err(e) => {
        tracing::debug!(event = "storage_index_skip", error = %e, "Не удалось прочитать storage_chat_id");
        None
      }
    };
    let Some(storage_chat_id) = storage_chat_id else { return; };
    if storage_chat_id != chat_id {
      return;
    }

    let tg = match state.telegram() {
      Ok(tg) => tg,
      Err(e) => {
        tracing::debug!(event = "storage_index_skip", error = %e, "Telegram сервис еще не готов");
        return;
      }
    };

    let mut unassigned = None;
    match indexer::index_storage_message(pool, tg.as_ref(), storage_chat_id, &msg, &mut unassigned).await {
      Ok(outcome) => {
        if outcome.dir || outcome.file || outcome.imported {
          let _ = app.emit("tree_updated", ());
        }
      }
      Err(e) => {
        tracing::warn!(event = "storage_index_failed", error = %e, "Не удалось обработать обновление");
      }
    }

    if msg.id > 0 {
      let current = sync::get_sync(pool, "storage_last_message_id")
        .await
        .ok()
        .and_then(|v| v.and_then(|s| s.parse::<i64>().ok()))
        .unwrap_or(0);
      if msg.id > current {
        let _ = sync::set_sync(pool, "storage_last_message_id", &msg.id.to_string()).await;
      }
    }
  });
}

fn file_ref_from_obj(obj: &serde_json::Map<String, Value>) -> Option<(i64, Option<String>)> {
  let id = obj.get("id").and_then(|v| v.as_i64())?;
  let remote_id = obj
    .get("remote")
    .and_then(|v| v.get("id"))
    .and_then(|v| v.as_str())
    .map(|s| s.to_string());
  Some((id, remote_id))
}

fn extract_file_ref_from_content(content: &Value) -> Option<(i64, Option<String>)> {
  let t = content.get("@type").and_then(|v| v.as_str()).unwrap_or("");
  let pick_nested = |parent: &Value, key: &str, nested: &str| {
    parent
      .get(key)
      .and_then(|v| v.as_object())
      .and_then(|o| o.get(nested))
      .and_then(|v| v.as_object())
      .and_then(file_ref_from_obj)
  };

  let by_type = match t {
    "messageDocument" => pick_nested(content, "document", "document")
      .or_else(|| content.get("document").and_then(|v| v.as_object()).and_then(file_ref_from_obj)),
    "messageVideo" => pick_nested(content, "video", "video")
      .or_else(|| content.get("video").and_then(|v| v.as_object()).and_then(file_ref_from_obj)),
    "messageAudio" => pick_nested(content, "audio", "audio")
      .or_else(|| content.get("audio").and_then(|v| v.as_object()).and_then(file_ref_from_obj)),
    "messageVoiceNote" => pick_nested(content, "voice_note", "voice")
      .or_else(|| content.get("voice_note").and_then(|v| v.as_object()).and_then(file_ref_from_obj)),
    "messageVideoNote" => pick_nested(content, "video_note", "video")
      .or_else(|| content.get("video_note").and_then(|v| v.as_object()).and_then(file_ref_from_obj)),
    "messageAnimation" => pick_nested(content, "animation", "animation")
      .or_else(|| content.get("animation").and_then(|v| v.as_object()).and_then(file_ref_from_obj)),
    "messageSticker" => pick_nested(content, "sticker", "sticker")
      .or_else(|| content.get("sticker").and_then(|v| v.as_object()).and_then(file_ref_from_obj)),
    _ => None
  };

  if by_type.is_some() {
    return by_type;
  }

  fn find_file_ref(value: &Value) -> Option<(i64, Option<String>)> {
    match value {
      Value::Object(map) => {
        if let Some(found) = file_ref_from_obj(map) {
          return Some(found);
        }
        for (k, v) in map {
          if k == "thumbnail" || k == "minithumbnail" {
            continue;
          }
          if let Some(found) = find_file_ref(v) {
            return Some(found);
          }
        }
      }
      Value::Array(arr) => {
        for v in arr {
          if let Some(found) = find_file_ref(v) {
            return Some(found);
          }
        }
      }
      _ => {}
    }
    None
  }

  find_file_ref(content)
}

fn extract_active_username(value: &Value) -> Option<String> {
  if let Some(name) = value.get("username").and_then(|v| v.as_str()) {
    if !name.trim().is_empty() {
      return Some(name.to_string());
    }
  }
  if let Some(list) = value
    .get("usernames")
    .and_then(|v| v.get("active_usernames"))
    .and_then(|v| v.as_array())
  {
    for item in list {
      if let Some(name) = item.as_str() {
        if !name.trim().is_empty() {
          return Some(name.to_string());
        }
      }
    }
  }
  None
}

fn local_path_from_file(value: &Value) -> Option<String> {
  let local = value.get("local").and_then(|v| v.as_object())?;
  let done = local.get("is_downloading_completed").and_then(|v| v.as_bool()).unwrap_or(false);
  let path = local.get("path").and_then(|v| v.as_str()).unwrap_or("");
  if done && !path.trim().is_empty() {
    Some(path.to_string())
  } else {
    None
  }
}

async fn chat_info_from_id(tg: &TdlibTelegram, chat_id: i64) -> Option<ChatInfo> {
  let chat = tg.request(json!({"@type":"getChat","chat_id":chat_id}), Duration::from_secs(10)).await.ok()?;
  let title = chat.get("title").and_then(|v| v.as_str()).unwrap_or("Без названия").to_string();
  let chat_type = chat.get("type").and_then(|v| v.as_object());
  let type_name = chat_type
    .and_then(|t| t.get("@type"))
    .and_then(|v| v.as_str())
    .unwrap_or("");
  let mut kind = "чат".to_string();
  let mut username = extract_active_username(&chat);

  if type_name == "chatTypeSupergroup" {
    let is_channel = chat_type
      .and_then(|t| t.get("is_channel"))
      .and_then(|v| v.as_bool())
      .unwrap_or(false);
    kind = if is_channel { "канал" } else { "группа" }.to_string();
    if username.is_none() {
      if let Some(supergroup_id) = chat_type.and_then(|t| t.get("supergroup_id")).and_then(|v| v.as_i64()) {
        if let Ok(sg) = tg.request(json!({"@type":"getSupergroup","supergroup_id":supergroup_id}), Duration::from_secs(10)).await {
          username = extract_active_username(&sg);
        }
      }
    }
  } else if type_name == "chatTypeBasicGroup" {
    kind = "группа".to_string();
  } else if type_name == "chatTypePrivate" {
    kind = "личный чат".to_string();
  }

  Some(ChatInfo { id: chat_id, title, kind, username })
}

impl TdlibTelegram {
  pub fn new(
    paths: Paths,
    app: tauri::AppHandle,
    initial_settings: Option<TgCredentials>,
    initial_tdlib_path: Option<String>
  ) -> anyhow::Result<Self> {
    let (tx, rx) = mpsc::channel::<TdlibCommand>();
    let send_waiters: SendWaiters = std::sync::Arc::new(Mutex::new(HashMap::new()));
    let send_results: SendResults = std::sync::Arc::new(Mutex::new(HashMap::new()));

    let app_for_thread = app.clone();
    let paths_for_thread = paths.clone();
    let waiters_for_thread = send_waiters.clone();
    let results_for_thread = send_results.clone();
    let session_name = tdlib_session_name();
    let mut config = match initial_settings {
      Some(s) => Some(TdlibConfig::from_settings(&paths, s.api_id, s.api_hash, &session_name)?),
      None => None
    };
    let mut lib_path = resolve_tdlib_path(&paths, initial_tdlib_path.as_deref());

    std::thread::spawn(move || {
      let mut last_state: Option<AuthState> = None;
      let mut waiting_for_params = false;
      let mut params_sent = false;
      let mut client: Option<TdlibClient> = None;
      let mut pending: Vec<String> = Vec::new();
      let mut build_attempted = false;
      let mut pending_requests: PendingRequests = HashMap::new();
      let mut next_request_id: u64 = 1;

      if config.is_none() || lib_path.is_none() {
        set_auth_state(&app_for_thread, AuthState::WaitConfig, &mut last_state);
      }

      loop {
        match rx.recv_timeout(Duration::from_millis(10)) {
          Ok(cmd) => {
            let mut cmd_ctx = CommandCtx {
              paths: &paths_for_thread,
              config: &mut config,
              lib_path: &mut lib_path,
              client: &mut client,
              waiting_for_params: &mut waiting_for_params,
              params_sent: &mut params_sent,
              pending_requests: &mut pending_requests,
              next_request_id: &mut next_request_id,
              build_attempted: &mut build_attempted,
              pending: &mut pending,
              app: &app_for_thread,
              last_state: &mut last_state
            };
            handle_command(cmd, &mut cmd_ctx);
          }
          Err(mpsc::RecvTimeoutError::Timeout) => {}
          Err(mpsc::RecvTimeoutError::Disconnected) => break
        }

        while let Ok(cmd) = rx.try_recv() {
          let mut cmd_ctx = CommandCtx {
            paths: &paths_for_thread,
            config: &mut config,
            lib_path: &mut lib_path,
            client: &mut client,
            waiting_for_params: &mut waiting_for_params,
            params_sent: &mut params_sent,
            pending_requests: &mut pending_requests,
            next_request_id: &mut next_request_id,
            build_attempted: &mut build_attempted,
            pending: &mut pending,
            app: &app_for_thread,
            last_state: &mut last_state
          };
          handle_command(cmd, &mut cmd_ctx);
        }

        if client.is_none() {
          if config.is_some() && lib_path.is_none() && !build_attempted {
            build_attempted = true;
            match attempt_tdlib_download(&paths_for_thread, &app_for_thread) {
              Ok(Some(p)) => {
                lib_path = Some(p);
              }
              Ok(None) => {}
              Err(e) => {
                tracing::warn!("Автоскачивание TDLib не удалось: {e}");
                emit_build_log(&app_for_thread, "stderr", &format!("Ошибка загрузки TDLib: {e}"));
              }
            }

            if lib_path.is_none() {
              match attempt_tdlib_build(&paths_for_thread, &app_for_thread) {
                Ok(p) => {
                  lib_path = Some(p);
                }
                Err(e) => {
                  tracing::error!("Автосборка TDLib не удалась: {e}");
                  set_auth_state(&app_for_thread, AuthState::WaitConfig, &mut last_state);
                }
              }
            }
          }

          if let (Some(_cfg), Some(lp)) = (config.as_ref(), lib_path.as_ref()) {
            match TdlibClient::load(lp) {
              Ok(c) => {
                c.set_verbosity(2);
                let _ = c.send(&json!({"@type":"getAuthorizationState"}).to_string());
                for msg in pending.drain(..) {
                  let _ = c.send(&msg);
                }
                client = Some(c);
                waiting_for_params = false;
                params_sent = false;
              }
              Err(e) => {
                tracing::error!(tdlib_path = %lp.display(), error = %e, "Не удалось загрузить TDLib");
                set_auth_state(&app_for_thread, AuthState::WaitConfig, &mut last_state);
              }
            }
          }
        }

        if let Some(c) = client.as_ref() {
          if let Some(resp) = c.receive(0.1) {
            let value: Value = match serde_json::from_str(&resp) {
              Ok(v) => v,
              Err(e) => {
                tracing::error!("Не удалось распарсить ответ TDLib: {e}");
                continue;
              }
            };
            if handle_request_response(&value, &mut pending_requests) {
              continue;
            }
            let mut response_ctx = ResponseCtx {
              client: c,
              config: &mut config,
              waiting_for_params: &mut waiting_for_params,
              params_sent: &mut params_sent,
              app: &app_for_thread,
              last_state: &mut last_state,
              send_waiters: &waiters_for_thread,
              send_results: &results_for_thread
            };
            if let Err(e) = handle_tdlib_response(&value, &mut response_ctx) {
              tracing::error!("Ошибка TDLib: {e}");
            }
          }
        }
      }

      if let Some(c) = client {
        c.destroy();
      }
    });

    Ok(Self { tx, paths, send_waiters, send_results })
  }

  async fn request(&self, payload: Value, timeout: Duration) -> Result<Value, TgError> {
    let (tx, rx) = oneshot::channel();
    self
      .tx
      .send(TdlibCommand::Request { payload, respond_to: tx })
      .map_err(|_| TgError::Other("TDLib поток не запущен".into()))?;

    match tokio::time::timeout(timeout, rx).await {
      Ok(Ok(Ok(v))) => Ok(v),
      Ok(Ok(Err(e))) => Err(TgError::Other(e.to_string())),
      Ok(Err(_)) => Err(TgError::Other("TDLib не вернул ответ".into())),
      Err(_) => Err(TgError::Other("Таймаут ответа TDLib".into()))
    }
  }

  async fn ensure_authorized(&self) -> Result<(), TgError> {
    let state = self
      .request(json!({"@type":"getAuthorizationState"}), Duration::from_secs(10))
      .await?;
    let t = state.get("@type").and_then(|v| v.as_str()).unwrap_or("");
    if t != "authorizationStateReady" {
      return Err(TgError::AuthRequired);
    }
    Ok(())
  }

  async fn is_supergroup_usable(&self, supergroup_id: i64) -> Result<bool, TgError> {
    if supergroup_id == 0 {
      return Ok(false);
    }
    let sg = self
      .request(json!({"@type":"getSupergroup","supergroup_id":supergroup_id}), Duration::from_secs(10))
      .await?;
    let status = sg
      .get("status")
      .and_then(|v| v.get("@type"))
      .and_then(|v| v.as_str())
      .unwrap_or("");
    if status == "chatMemberStatusLeft" || status == "chatMemberStatusBanned" {
      return Ok(false);
    }
    Ok(true)
  }

  async fn find_storage_channel(&self) -> Result<Option<ChatId>, TgError> {
    let mut chat_ids: Vec<ChatId> = Vec::new();

    for title in [storage_channel_title(), storage_channel_title_legacy()] {
      if let Ok(res) = self
        .request(json!({"@type":"searchChats","query":title,"limit":20}), Duration::from_secs(10))
        .await
      {
        if let Some(list) = res.get("chat_ids").and_then(|v| v.as_array()) {
          for id in list {
            if let Some(v) = id.as_i64() {
              if !chat_ids.contains(&v) {
                chat_ids.push(v);
              }
            }
          }
        }
      }
    }

    if chat_ids.is_empty() {
      for title in [storage_channel_title(), storage_channel_title_legacy()] {
        if let Ok(res) = self
          .request(json!({"@type":"searchChatsOnServer","query":title,"limit":20}), Duration::from_secs(10))
          .await
        {
          if let Some(list) = res.get("chat_ids").and_then(|v| v.as_array()) {
            for id in list {
              if let Some(v) = id.as_i64() {
                if !chat_ids.contains(&v) {
                  chat_ids.push(v);
                }
              }
            }
          }
        }
      }
    }

    let mut fallback: Option<ChatId> = None;
    for chat_id in chat_ids {
      let chat = self
        .request(json!({"@type":"getChat","chat_id":chat_id}), Duration::from_secs(10))
        .await?;
      let title = chat.get("title").and_then(|v| v.as_str()).unwrap_or("");
      let chat_type = chat.get("type").and_then(|v| v.as_object());
      let is_channel = chat_type
        .and_then(|t| t.get("is_channel"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
      let type_name = chat_type
        .and_then(|t| t.get("@type"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
      if type_name == "chatTypeSupergroup" && is_channel {
        let supergroup_id = chat_type
          .and_then(|t| t.get("supergroup_id"))
          .and_then(|v| v.as_i64())
          .unwrap_or(0);
        if !self.is_supergroup_usable(supergroup_id).await? {
          tracing::warn!(event = "storage_channel_unusable", chat_id = chat_id, "Канал хранения недоступен, пропускаю");
          let _ = self
            .request(
              json!({"@type":"deleteChatHistory","chat_id":chat_id,"remove_from_chat_list":true,"revoke":true}),
              Duration::from_secs(10)
            )
            .await;
          let _ = self
            .request(json!({"@type":"leaveChat","chat_id":chat_id}), Duration::from_secs(10))
            .await;
          continue;
        }

        if title == storage_channel_title() {
          return Ok(Some(chat_id));
        }
        if title == storage_channel_title_legacy() {
          fallback = Some(chat_id);
        }
      }
    }

    Ok(fallback)
  }

  async fn find_backup_channel(&self) -> Result<Option<ChatId>, TgError> {
    let mut chat_ids: Vec<ChatId> = Vec::new();

    if let Ok(res) = self
      .request(json!({"@type":"searchChats","query":backup_channel_title(),"limit":20}), Duration::from_secs(10))
      .await
    {
      if let Some(list) = res.get("chat_ids").and_then(|v| v.as_array()) {
        for id in list {
          if let Some(v) = id.as_i64() {
            if !chat_ids.contains(&v) {
              chat_ids.push(v);
            }
          }
        }
      }
    }

    if chat_ids.is_empty() {
      if let Ok(res) = self
        .request(json!({"@type":"searchChatsOnServer","query":backup_channel_title(),"limit":20}), Duration::from_secs(10))
        .await
      {
        if let Some(list) = res.get("chat_ids").and_then(|v| v.as_array()) {
          for id in list {
            if let Some(v) = id.as_i64() {
              if !chat_ids.contains(&v) {
                chat_ids.push(v);
              }
            }
          }
        }
      }
    }

    for chat_id in chat_ids {
      let chat = self
        .request(json!({"@type":"getChat","chat_id":chat_id}), Duration::from_secs(10))
        .await?;
      let title = chat.get("title").and_then(|v| v.as_str()).unwrap_or("");
      let chat_type = chat.get("type").and_then(|v| v.as_object());
      let is_channel = chat_type
        .and_then(|t| t.get("is_channel"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
      let type_name = chat_type
        .and_then(|t| t.get("@type"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
      if type_name == "chatTypeSupergroup" && is_channel {
        let supergroup_id = chat_type
          .and_then(|t| t.get("supergroup_id"))
          .and_then(|v| v.as_i64())
          .unwrap_or(0);
        if !self.is_supergroup_usable(supergroup_id).await? {
          tracing::warn!(event = "backup_channel_unusable", chat_id = chat_id, "Канал бэкапов недоступен, пропускаю");
          let _ = self
            .request(
              json!({"@type":"deleteChatHistory","chat_id":chat_id,"remove_from_chat_list":true,"revoke":true}),
              Duration::from_secs(10)
            )
            .await;
          let _ = self
            .request(json!({"@type":"leaveChat","chat_id":chat_id}), Duration::from_secs(10))
            .await;
          continue;
        }

        if title == backup_channel_title() {
          return Ok(Some(chat_id));
        }
      }
    }

    Ok(None)
  }

  async fn create_storage_channel(&self) -> Result<ChatId, TgError> {
    let chat = self
      .request(
        json!({
          "@type":"createNewSupergroupChat",
          "title": storage_channel_title(),
          "is_channel":true,
          "description":"Хранилище CloudTG"
        }),
        Duration::from_secs(20)
      )
      .await?;
    let chat_id = chat
      .get("id")
      .and_then(|v| v.as_i64())
      .ok_or_else(|| TgError::Other("TDLib не вернул chat_id".into()))?;
    Ok(chat_id)
  }

  async fn create_backup_channel(&self) -> Result<ChatId, TgError> {
    let chat = self
      .request(
        json!({
          "@type":"createNewSupergroupChat",
          "title": backup_channel_title(),
          "is_channel":true,
          "description":"Бэкапы CloudTG"
        }),
        Duration::from_secs(20)
      )
      .await?;
    let chat_id = chat
      .get("id")
      .and_then(|v| v.as_i64())
      .ok_or_else(|| TgError::Other("TDLib не вернул chat_id".into()))?;
    Ok(chat_id)
  }

  fn ensure_icon_file(&self) -> Result<PathBuf, TgError> {
    let icon_path = self.paths.cache_dir.join("cloudtg-icon-telegram.png");
    let bytes = match build_telegram_icon(app_icon_bytes()) {
      Ok(icon) => icon,
      Err(e) => {
        tracing::warn!(event = "storage_channel_icon_prepare_failed", error = %e, "Не удалось подготовить иконку канала");
        app_icon_bytes().to_vec()
      }
    };
    if let Ok(existing) = std::fs::read(&icon_path) {
      if existing == bytes {
        return Ok(icon_path);
      }
    }
    if let Some(parent) = icon_path.parent() {
      std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&icon_path, bytes)?;
    Ok(icon_path)
  }

  async fn ensure_storage_channel_config(&self, chat_id: ChatId) -> Result<(), TgError> {
    let chat = self
      .request(json!({"@type":"getChat","chat_id":chat_id}), Duration::from_secs(10))
      .await?;
    let title = chat.get("title").and_then(|v| v.as_str()).unwrap_or("");
    if title != storage_channel_title() {
      let _ = self
        .request(
          json!({"@type":"setChatTitle","chat_id":chat_id,"title":storage_channel_title()}),
          Duration::from_secs(10)
        )
        .await?;
      tracing::info!(event = "storage_channel_title_updated", chat_id = chat_id, "Название канала обновлено");
    }

    if let Ok(icon_path) = self.ensure_icon_file() {
      let path_str = icon_path.to_string_lossy().to_string();
      let _ = self
        .request(
          json!({
            "@type":"setChatPhoto",
            "chat_id": chat_id,
            "photo": {
              "@type":"inputChatPhotoStatic",
              "photo": { "@type":"inputFileLocal", "path": path_str }
            }
          }),
          Duration::from_secs(20)
        )
        .await?;
      tracing::info!(event = "storage_channel_photo_updated", chat_id = chat_id, "Иконка канала обновлена");
    }

    let _ = self
      .request(
        json!({
          "@type":"setChatNotificationSettings",
          "chat_id": chat_id,
          "notification_settings": {
            "@type":"chatNotificationSettings",
            "use_default_mute_for": false,
            "mute_for": 0,
            "use_default_sound": true,
            "sound_id": 0,
            "use_default_show_preview": true,
            "show_preview": true,
            "use_default_disable_pinned_message_notifications": true,
            "disable_pinned_message_notifications": false,
            "use_default_disable_mention_notifications": true,
            "disable_mention_notifications": false
          }
        }),
        Duration::from_secs(10)
      )
      .await?;
    tracing::info!(event = "storage_channel_notifications_enabled", chat_id = chat_id, "Уведомления канала включены");

    Ok(())
  }

  async fn ensure_backup_channel_config(&self, chat_id: ChatId) -> Result<(), TgError> {
    let chat = self
      .request(json!({"@type":"getChat","chat_id":chat_id}), Duration::from_secs(10))
      .await?;
    let title = chat.get("title").and_then(|v| v.as_str()).unwrap_or("");
    if title != backup_channel_title() {
      let _ = self
        .request(
          json!({"@type":"setChatTitle","chat_id":chat_id,"title":backup_channel_title()}),
          Duration::from_secs(10)
        )
        .await?;
      tracing::info!(event = "backup_channel_title_updated", chat_id = chat_id, "Название канала обновлено");
    }

    if let Ok(icon_path) = self.ensure_icon_file() {
      let path_str = icon_path.to_string_lossy().to_string();
      let _ = self
        .request(
          json!({
            "@type":"setChatPhoto",
            "chat_id": chat_id,
            "photo": {
              "@type":"inputChatPhotoStatic",
              "photo": { "@type":"inputFileLocal", "path": path_str }
            }
          }),
          Duration::from_secs(20)
        )
        .await?;
      tracing::info!(event = "backup_channel_photo_updated", chat_id = chat_id, "Иконка канала обновлена");
    }

    let _ = self
      .request(
        json!({
          "@type":"setChatNotificationSettings",
          "chat_id": chat_id,
          "notification_settings": {
            "@type":"chatNotificationSettings",
            "use_default_mute_for": false,
            "mute_for": 0,
            "use_default_sound": true,
            "sound_id": 0,
            "use_default_show_preview": true,
            "show_preview": true,
            "use_default_disable_pinned_message_notifications": true,
            "disable_pinned_message_notifications": false,
            "use_default_disable_mention_notifications": true,
            "disable_mention_notifications": false
          }
        }),
        Duration::from_secs(10)
      )
      .await?;
    tracing::info!(event = "backup_channel_notifications_enabled", chat_id = chat_id, "Уведомления канала включены");

    Ok(())
  }
}

#[async_trait::async_trait]
impl TelegramService for TdlibTelegram {
  async fn auth_start(&self, phone: String) -> Result<(), TgError> {
    let payload = json!({
      "@type": "setAuthenticationPhoneNumber",
      "phone_number": phone,
      "settings": {
        "allow_flash_call": false,
        "is_current_phone_number": false,
        "allow_sms_retriever_api": false
      }
    })
    .to_string();

    self.tx
      .send(TdlibCommand::Td(payload))
      .map_err(|_| TgError::Other("TDLib поток не запущен".into()))?;
    Ok(())
  }

  async fn auth_submit_code(&self, code: String) -> Result<(), TgError> {
    let payload = json!({"@type":"checkAuthenticationCode","code":code}).to_string();
    self.tx
      .send(TdlibCommand::Td(payload))
      .map_err(|_| TgError::Other("TDLib поток не запущен".into()))?;
    Ok(())
  }

  async fn auth_submit_password(&self, password: String) -> Result<(), TgError> {
    let payload = json!({"@type":"checkAuthenticationPassword","password":password}).to_string();
    self.tx
      .send(TdlibCommand::Td(payload))
      .map_err(|_| TgError::Other("TDLib поток не запущен".into()))?;
    Ok(())
  }

  async fn configure(&self, api_id: i32, api_hash: String, tdlib_path: Option<String>) -> Result<(), TgError> {
    self.tx
      .send(TdlibCommand::SetConfig { api_id, api_hash, tdlib_path })
      .map_err(|_| TgError::Other("TDLib поток не запущен".into()))?;
    Ok(())
  }

  async fn storage_check_channel(&self, chat_id: ChatId) -> Result<bool, TgError> {
    self.ensure_authorized().await?;
    let chat = self
      .request(json!({"@type":"getChat","chat_id":chat_id}), Duration::from_secs(10))
      .await?;
    let title = chat.get("title").and_then(|v| v.as_str()).unwrap_or("");
    if title != storage_channel_title() && title != storage_channel_title_legacy() {
      return Ok(false);
    }
    let chat_type = chat.get("type").and_then(|v| v.as_object());
    let is_channel = chat_type
      .and_then(|t| t.get("is_channel"))
      .and_then(|v| v.as_bool())
      .unwrap_or(false);
    let type_name = chat_type
      .and_then(|t| t.get("@type"))
      .and_then(|v| v.as_str())
      .unwrap_or("");
    if type_name != "chatTypeSupergroup" || !is_channel {
      return Ok(false);
    }
    let supergroup_id = chat_type
      .and_then(|t| t.get("supergroup_id"))
      .and_then(|v| v.as_i64())
      .unwrap_or(0);
    self.is_supergroup_usable(supergroup_id).await
  }

  async fn backup_check_channel(&self, chat_id: ChatId) -> Result<bool, TgError> {
    self.ensure_authorized().await?;
    let chat = self
      .request(json!({"@type":"getChat","chat_id":chat_id}), Duration::from_secs(10))
      .await?;
    let title = chat.get("title").and_then(|v| v.as_str()).unwrap_or("");
    if title != backup_channel_title() {
      return Ok(false);
    }
    let chat_type = chat.get("type").and_then(|v| v.as_object());
    let is_channel = chat_type
      .and_then(|t| t.get("is_channel"))
      .and_then(|v| v.as_bool())
      .unwrap_or(false);
    let type_name = chat_type
      .and_then(|t| t.get("@type"))
      .and_then(|v| v.as_str())
      .unwrap_or("");
    if type_name != "chatTypeSupergroup" || !is_channel {
      return Ok(false);
    }
    let supergroup_id = chat_type
      .and_then(|t| t.get("supergroup_id"))
      .and_then(|v| v.as_i64())
      .unwrap_or(0);
    self.is_supergroup_usable(supergroup_id).await
  }

  async fn storage_get_or_create_channel(&self) -> Result<ChatId, TgError> {
    self.ensure_authorized().await?;
    tracing::info!(event = "storage_get_or_create_channel", "Поиск канала хранения");

    let chat_id = if let Some(id) = self.find_storage_channel().await? {
      tracing::info!(event = "storage_channel_found", chat_id = id, "Найден существующий канал хранения");
      id
    } else {
      tracing::info!(event = "storage_channel_create", "Создаю канал хранения");
      let id = self.create_storage_channel().await?;
      tracing::info!(event = "storage_channel_created", chat_id = id, "Канал хранения создан");
      id
    };

    if let Err(e) = self.ensure_storage_channel_config(chat_id).await {
      tracing::warn!(event = "storage_channel_config_failed", chat_id = chat_id, error = %e, "Не удалось обновить настройки канала");
    }

    Ok(chat_id)
  }

  async fn backup_get_or_create_channel(&self) -> Result<ChatId, TgError> {
    self.ensure_authorized().await?;
    tracing::info!(event = "backup_get_or_create_channel", "Поиск канала бэкапов");

    let chat_id = if let Some(id) = self.find_backup_channel().await? {
      tracing::info!(event = "backup_channel_found", chat_id = id, "Найден существующий канал бэкапов");
      id
    } else {
      tracing::info!(event = "backup_channel_create", "Создаю канал бэкапов");
      let id = self.create_backup_channel().await?;
      tracing::info!(event = "backup_channel_created", chat_id = id, "Канал бэкапов создан");
      id
    };

    if let Err(e) = self.ensure_backup_channel_config(chat_id).await {
      tracing::warn!(event = "backup_channel_config_failed", chat_id = chat_id, error = %e, "Не удалось обновить настройки канала");
    }

    Ok(chat_id)
  }

  async fn storage_create_channel(&self) -> Result<ChatId, TgError> {
    self.ensure_authorized().await?;
    tracing::info!(event = "storage_channel_create_manual", "Создаю новый канал хранения по запросу");
    let chat_id = self.create_storage_channel().await?;
    if let Err(e) = self.ensure_storage_channel_config(chat_id).await {
      tracing::warn!(event = "storage_channel_config_failed", chat_id = chat_id, error = %e, "Не удалось обновить настройки канала");
    }
    Ok(chat_id)
  }

  async fn storage_delete_channel(&self, chat_id: ChatId) -> Result<(), TgError> {
    self.ensure_authorized().await?;
    tracing::info!(event = "storage_channel_delete", chat_id = chat_id, "Удаление старого канала хранения");

    let chat = self
      .request(json!({"@type":"getChat","chat_id":chat_id}), Duration::from_secs(10))
      .await?;
    let chat_type = chat.get("type").and_then(|v| v.as_object());
    let is_channel = chat_type
      .and_then(|t| t.get("is_channel"))
      .and_then(|v| v.as_bool())
      .unwrap_or(false);
    let type_name = chat_type
      .and_then(|t| t.get("@type"))
      .and_then(|v| v.as_str())
      .unwrap_or("");

    if type_name == "chatTypeSupergroup" && is_channel {
      let supergroup_id = chat_type
        .and_then(|t| t.get("supergroup_id"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
      if supergroup_id != 0 {
        let can_delete = self.is_supergroup_usable(supergroup_id).await.unwrap_or(false);
        if can_delete {
          if let Err(e) = self
            .request(json!({"@type":"deleteSupergroup","supergroup_id":supergroup_id}), Duration::from_secs(10))
            .await
          {
            tracing::warn!(event = "storage_channel_delete_failed", chat_id = chat_id, error = %e, "Не удалось удалить канал, пробую выйти");
          } else {
            return Ok(());
          }
        }
      }
    }

    let _ = self
      .request(
        json!({"@type":"deleteChatHistory","chat_id":chat_id,"remove_from_chat_list":true,"revoke":true}),
        Duration::from_secs(10)
      )
      .await;
    let _ = self
      .request(json!({"@type":"leaveChat","chat_id":chat_id}), Duration::from_secs(10))
      .await;
    Ok(())
  }

  async fn chat_history(&self, chat_id: ChatId, from_message_id: MessageId, limit: i32)
    -> Result<SearchMessagesResult, TgError> {
    self.ensure_authorized().await?;
    let offset = if from_message_id == 0 { 0 } else { -1 };
    let res = self
      .request(
        json!({
          "@type":"getChatHistory",
          "chat_id": chat_id,
          "from_message_id": from_message_id,
          "offset": offset,
          "limit": limit,
          "only_local": false
        }),
        Duration::from_secs(30)
      )
      .await?;

    let mut messages = Vec::new();
    if let Some(list) = res.get("messages").and_then(|v| v.as_array()) {
      for m in list {
        let id = m.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
        if id == 0 {
          continue;
        }
        let date = m.get("date").and_then(|v| v.as_i64()).unwrap_or(0);
        let (text, caption, file_size, file_name) = if let Some(content) = m.get("content") {
          (extract_text(content), extract_caption(content), extract_file_size(content), extract_file_name(content))
        } else {
          (None, None, None, None)
        };
        messages.push(HistoryMessage { id, date, text, caption, file_size, file_name });
      }
    }
    let next_from_message_id = messages.last().map(|m| m.id).unwrap_or(0);

    Ok(SearchMessagesResult { total_count: None, next_from_message_id, messages })
  }

  async fn search_chat_messages(&self, chat_id: ChatId, query: String, from_message_id: MessageId, limit: i32)
    -> Result<SearchMessagesResult, TgError> {
    self.ensure_authorized().await?;
    let res = self
      .request(
        json!({
          "@type":"searchChatMessages",
          "chat_id": chat_id,
          "query": query,
          "from_message_id": from_message_id,
          "offset": 0,
          "limit": limit,
          "filter": null,
          "sender_id": null,
          "topic_id": null
        }),
        Duration::from_secs(30)
      )
      .await?;

    let total_count = res
      .get("total_count")
      .and_then(|v| v.as_i64())
      .and_then(|v| if v < 0 { None } else { Some(v) });
    let next_from_message_id = res
      .get("next_from_message_id")
      .and_then(|v| v.as_i64())
      .unwrap_or(0);

    let mut messages = Vec::new();
    if let Some(list) = res.get("messages").and_then(|v| v.as_array()) {
      for m in list {
        let id = m.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
        if id == 0 {
          continue;
        }
        let date = m.get("date").and_then(|v| v.as_i64()).unwrap_or(0);
        let (text, caption, file_size, file_name) = if let Some(content) = m.get("content") {
          (extract_text(content), extract_caption(content), extract_file_size(content), extract_file_name(content))
        } else {
          (None, None, None, None)
        };
        messages.push(HistoryMessage { id, date, text, caption, file_size, file_name });
      }
    }

    Ok(SearchMessagesResult { total_count, next_from_message_id, messages })
  }

  async fn search_storage_messages(&self, chat_id: ChatId, from_message_id: MessageId, limit: i32)
    -> Result<SearchMessagesResult, TgError> {
    self.search_chat_messages(chat_id, "#ocltg".into(), from_message_id, limit).await
  }

  async fn search_chats(&self, query: String, limit: i32) -> Result<Vec<ChatInfo>, TgError> {
    self.ensure_authorized().await?;
    let q = query.trim().to_string();
    let mut out: Vec<ChatInfo> = Vec::new();
    let mut seen: std::collections::HashSet<i64> = std::collections::HashSet::new();

    let mut ids: Vec<i64> = Vec::new();
    if !q.is_empty() {
      if let Ok(res) = self.request(json!({"@type":"searchChats","query":q,"limit":limit}), Duration::from_secs(10)).await {
        if let Some(list) = res.get("chat_ids").and_then(|v| v.as_array()) {
          for id in list {
            if let Some(v) = id.as_i64() {
              ids.push(v);
            }
          }
        }
      }
      if ids.is_empty() {
        if let Ok(res) = self.request(json!({"@type":"searchChatsOnServer","query":q,"limit":limit}), Duration::from_secs(10)).await {
          if let Some(list) = res.get("chat_ids").and_then(|v| v.as_array()) {
            for id in list {
              if let Some(v) = id.as_i64() {
                ids.push(v);
              }
            }
          }
        }
      }
    }

    if q.starts_with('@') {
      let username = q.trim_start_matches('@');
      if let Ok(chat) = self.request(json!({"@type":"searchPublicChat","username":username}), Duration::from_secs(10)).await {
        if let Some(chat_id) = chat.get("id").and_then(|v| v.as_i64()) {
          ids.push(chat_id);
        }
      }
    }

    for chat_id in ids {
      if seen.contains(&chat_id) {
        continue;
      }
      if let Some(info) = chat_info_from_id(self, chat_id).await {
        seen.insert(chat_id);
        out.push(info);
      }
    }
    Ok(out)
  }

  async fn recent_chats(&self, limit: i32) -> Result<Vec<ChatInfo>, TgError> {
    self.ensure_authorized().await?;
    let mut out: Vec<ChatInfo> = Vec::new();
    let mut seen: std::collections::HashSet<i64> = std::collections::HashSet::new();

    if let Ok(me) = self.request(json!({"@type":"getMe"}), Duration::from_secs(10)).await {
      if let Some(user_id) = me.get("id").and_then(|v| v.as_i64()) {
        if let Ok(chat) = self.request(json!({"@type":"createPrivateChat","user_id":user_id,"force":true}), Duration::from_secs(10)).await {
          if let Some(chat_id) = chat.get("id").and_then(|v| v.as_i64()) {
            seen.insert(chat_id);
            out.push(ChatInfo {
              id: chat_id,
              title: "Избранное".to_string(),
              kind: "личный чат".to_string(),
              username: None
            });
          }
        }
      }
    }

    let res = self
      .request(json!({"@type":"getChats","chat_list":{"@type":"chatListMain"},"limit":limit}), Duration::from_secs(10))
      .await?;
    if let Some(list) = res.get("chat_ids").and_then(|v| v.as_array()) {
      for id in list {
        let chat_id = match id.as_i64() {
          Some(v) => v,
          None => continue
        };
        if seen.contains(&chat_id) {
          continue;
        }
        if let Some(info) = chat_info_from_id(self, chat_id).await {
          seen.insert(chat_id);
          out.push(info);
        }
      }
    }
    Ok(out)
  }

  async fn send_text_message(&self, chat_id: ChatId, text: String) -> Result<UploadedMessage, TgError> {
    self.ensure_authorized().await?;
    tracing::info!(event = "tdlib_send_text_message", chat_id = chat_id, "Отправка тестового сообщения");

    let res = self
      .request(
        json!({
          "@type":"sendMessage",
          "chat_id": chat_id,
          "input_message_content": {
            "@type":"inputMessageText",
            "text": { "@type":"formattedText", "text": text },
            "disable_web_page_preview": true,
            "clear_draft": false
          }
        }),
        Duration::from_secs(20)
      )
      .await?;

    let msg_id = res
      .get("id")
      .and_then(|v| v.as_i64())
      .ok_or_else(|| TgError::Other("TDLib не вернул message.id".into()))?;
    let chat_id = res
      .get("chat_id")
      .and_then(|v| v.as_i64())
      .unwrap_or(chat_id);

    tracing::info!(event = "tdlib_send_text_message_done", chat_id = chat_id, message_id = msg_id, "Тестовое сообщение отправлено");
    Ok(UploadedMessage { chat_id, message_id: msg_id, caption_or_text: text })
  }

  async fn send_dir_message(&self, chat_id: ChatId, text: String) -> Result<UploadedMessage, TgError> {
    self.ensure_authorized().await?;
    tracing::info!(event = "tdlib_send_dir_message", chat_id = chat_id, "Отправка сообщения директории");

    let res = self
      .request(
        json!({
          "@type":"sendMessage",
          "chat_id": chat_id,
          "input_message_content": {
            "@type":"inputMessageText",
            "text": { "@type":"formattedText", "text": text },
            "disable_web_page_preview": true,
            "clear_draft": false
          }
        }),
        Duration::from_secs(20)
      )
      .await?;

    let msg_id = res
      .get("id")
      .and_then(|v| v.as_i64())
      .ok_or_else(|| TgError::Other("TDLib не вернул message.id".into()))?;
    let chat_id = res
      .get("chat_id")
      .and_then(|v| v.as_i64())
      .unwrap_or(chat_id);

    tracing::info!(event = "tdlib_send_dir_message_done", chat_id = chat_id, message_id = msg_id, "Сообщение директории отправлено");
    Ok(UploadedMessage { chat_id, message_id: msg_id, caption_or_text: text })
  }

  async fn edit_message_text(&self, chat_id: ChatId, message_id: MessageId, text: String) -> Result<(), TgError> {
    self.ensure_authorized().await?;
    tracing::info!(event = "tdlib_edit_message", chat_id = chat_id, message_id = message_id, "Обновление текста сообщения");

    let _ = self
      .request(
        json!({
          "@type":"editMessageText",
          "chat_id": chat_id,
          "message_id": message_id,
          "input_message_content": {
            "@type":"inputMessageText",
            "text": { "@type":"formattedText", "text": text },
            "disable_web_page_preview": true,
            "clear_draft": false
          }
        }),
        Duration::from_secs(20)
      )
      .await?;

    Ok(())
  }

  async fn edit_message_caption(&self, chat_id: ChatId, message_id: MessageId, caption: String) -> Result<(), TgError> {
    self.ensure_authorized().await?;
    tracing::info!(event = "tdlib_edit_message_caption", chat_id = chat_id, message_id = message_id, "Обновление подписи сообщения");

    let _ = self
      .request(
        json!({
          "@type":"editMessageCaption",
          "chat_id": chat_id,
          "message_id": message_id,
          "caption": { "@type":"formattedText", "text": caption },
          "show_caption_above_media": false
        }),
        Duration::from_secs(20)
      )
      .await?;

    Ok(())
  }

  async fn send_file(&self, chat_id: ChatId, path: std::path::PathBuf, caption: String) -> Result<UploadedMessage, TgError> {
    self.ensure_authorized().await?;
    tracing::info!(event = "tdlib_send_file", chat_id = chat_id, "Отправка файла");

    let path_str = path.to_string_lossy().to_string();
    let res = self
      .request(
        json!({
          "@type":"sendMessage",
          "chat_id": chat_id,
          "input_message_content": {
            "@type":"inputMessageDocument",
            "document": { "@type":"inputFileLocal", "path": path_str },
            "caption": { "@type":"formattedText", "text": caption },
            "disable_content_type_detection": false
          }
        }),
        Duration::from_secs(60)
      )
      .await?;

    let msg_id = res
      .get("id")
      .and_then(|v| v.as_i64())
      .ok_or_else(|| TgError::Other("TDLib не вернул message.id".into()))?;
    let chat_id = res
      .get("chat_id")
      .and_then(|v| v.as_i64())
      .unwrap_or(chat_id);

    tracing::info!(event = "tdlib_send_file_done", chat_id = chat_id, message_id = msg_id, "Файл отправлен");
    Ok(UploadedMessage { chat_id, message_id: msg_id, caption_or_text: caption })
  }

  async fn send_file_from_message(&self, chat_id: ChatId, message_id: MessageId, caption: String) -> Result<UploadedMessage, TgError> {
    self.ensure_authorized().await?;
    tracing::info!(event = "tdlib_send_file_from_message", chat_id = chat_id, message_id = message_id, "Отправка файла из сообщения");

    let msg = self
      .request(
        json!({
          "@type":"getMessage",
          "chat_id": chat_id,
          "message_id": message_id
        }),
        Duration::from_secs(20)
      )
      .await?;

    let content = msg
      .get("content")
      .ok_or_else(|| TgError::Other("Не удалось получить содержимое сообщения".into()))?;
    let (file_id, remote_id) = extract_file_ref_from_content(content)
      .ok_or_else(|| TgError::Other("Не удалось получить файл из сообщения".into()))?;

    let send_with_input = |input, caption: String| async move {
      self
        .request(
          json!({
            "@type":"sendMessage",
            "chat_id": chat_id,
            "input_message_content": {
              "@type":"inputMessageDocument",
              "document": input,
              "caption": { "@type":"formattedText", "text": caption },
              "disable_content_type_detection": false
            }
          }),
          Duration::from_secs(60)
        )
        .await
    };

    let res = match send_with_input(json!({ "@type":"inputFileId", "id": file_id }), caption.clone()).await {
      Ok(v) => Ok(v),
      Err(first) => {
        if let Some(remote) = remote_id {
          match send_with_input(json!({ "@type":"inputFileRemote", "id": remote }), caption.clone()).await {
            Ok(v) => Ok(v),
            Err(second) => Err(TgError::Other(format!(
              "Не удалось отправить файл по id: {first}. По remote: {second}"
            )))
          }
        } else {
          Err(first)
        }
      }
    }?;

    let msg_id = res
      .get("id")
      .and_then(|v| v.as_i64())
      .ok_or_else(|| TgError::Other("TDLib не вернул message.id".into()))?;
    let chat_id = res
      .get("chat_id")
      .and_then(|v| v.as_i64())
      .unwrap_or(chat_id);

    let final_id = if msg_id > 0 {
      msg_id
    } else {
      let immediate = { self.send_results.lock().remove(&msg_id) };
      if let Some(result) = immediate {
        match result {
          Ok(id) if id > 0 => id,
          Ok(_) => return Err(TgError::Other("TDLib вернул некорректный id отправленного сообщения".into())),
          Err(err) => return Err(TgError::Other(err))
        }
      } else {
        let (tx, rx) = oneshot::channel();
        {
          let mut guard = self.send_waiters.lock();
          guard.insert(msg_id, tx);
        }
        match tokio::time::timeout(Duration::from_secs(20), rx).await {
          Ok(Ok(Ok(id))) if id > 0 => id,
          Ok(Ok(Ok(_))) => {
            return Err(TgError::Other("TDLib вернул некорректный id отправленного сообщения".into()));
          }
          Ok(Ok(Err(e))) => {
            return Err(TgError::Other(e.to_string()));
          }
          Ok(Err(_)) => {
            return Err(TgError::Other("TDLib не подтвердил отправку сообщения".into()));
          }
          Err(_) => {
            self.send_waiters.lock().remove(&msg_id);
            return Err(TgError::Other("Таймаут подтверждения отправки сообщения".into()));
          }
        }
      }
    };

    Ok(UploadedMessage { chat_id, message_id: final_id, caption_or_text: caption })
  }

  async fn forward_message(&self, from_chat_id: ChatId, to_chat_id: ChatId, message_id: MessageId) -> Result<MessageId, TgError> {
    self.ensure_authorized().await?;
    let res = self
      .request(
        json!({
          "@type":"forwardMessages",
          "chat_id": to_chat_id,
          "from_chat_id": from_chat_id,
          "message_ids": [message_id],
          "send_copy": false,
          "remove_caption": false,
          "options": {
            "@type": "messageSendOptions",
            "disable_notification": false,
            "from_background": false,
            "protect_content": false
          }
        }),
        Duration::from_secs(30)
      )
      .await?;
    if let Some(list) = res.get("messages").and_then(|v| v.as_array()) {
      if let Some(first) = list.first() {
        if first.is_null() {
          return Err(TgError::Other("TDLib не вернул сообщение при пересылке".into()));
        }
        if let Some(id) = first.get("id").and_then(|v| v.as_i64()) {
          return Ok(id);
        }
      }
    }
    Err(TgError::Other("TDLib не вернул сообщение при пересылке".into()))
  }

  async fn copy_messages(
    &self,
    from_chat_id: ChatId,
    to_chat_id: ChatId,
    message_ids: Vec<MessageId>
  ) -> Result<Vec<Option<MessageId>>, TgError> {
    self.ensure_authorized().await?;
    if message_ids.is_empty() {
      return Ok(Vec::new());
    }
    let res = self
      .request(
        json!({
          "@type":"forwardMessages",
          "chat_id": to_chat_id,
          "from_chat_id": from_chat_id,
          "message_ids": message_ids,
          "send_copy": true,
          "remove_caption": false,
          "options": {
            "@type": "messageSendOptions",
            "disable_notification": false,
            "from_background": false,
            "protect_content": false
          }
        }),
        Duration::from_secs(30)
      )
      .await?;
    let mut out = Vec::new();
    if let Some(list) = res.get("messages").and_then(|v| v.as_array()) {
      for m in list {
        if m.is_null() {
          out.push(None);
        } else {
          let id = m.get("id").and_then(|v| v.as_i64());
          out.push(id);
        }
      }
      return Ok(out);
    }
    Err(TgError::Other("TDLib не вернул список сообщений при копировании".into()))
  }

  async fn delete_messages(&self, chat_id: ChatId, message_ids: Vec<MessageId>, revoke: bool) -> Result<(), TgError> {
    self.ensure_authorized().await?;
    if message_ids.is_empty() {
      return Ok(());
    }
    let _ = self
      .request(
        json!({
          "@type":"deleteMessages",
          "chat_id": chat_id,
          "message_ids": message_ids,
          "revoke": revoke
        }),
        Duration::from_secs(20)
      )
      .await?;
    Ok(())
  }

  async fn download_message_file(&self, chat_id: ChatId, message_id: MessageId, target: std::path::PathBuf)
    -> Result<std::path::PathBuf, TgError> {
    self.ensure_authorized().await?;
    let msg = self
      .request(
        json!({
          "@type":"getMessage",
          "chat_id": chat_id,
          "message_id": message_id
        }),
        Duration::from_secs(20)
      )
      .await?;

    let content = msg
      .get("content")
      .ok_or_else(|| TgError::Other("Не удалось получить содержимое сообщения".into()))?;
    let (file_id, _) = extract_file_ref_from_content(content)
      .ok_or_else(|| TgError::Other("Не удалось получить файл из сообщения".into()))?;

    let downloaded = self
      .request(
        json!({
          "@type":"downloadFile",
          "file_id": file_id,
          "priority": 1,
          "offset": 0,
          "limit": 0,
          "synchronous": true
        }),
        Duration::from_secs(60)
      )
      .await?;

    let mut local_path = local_path_from_file(&downloaded);
    if local_path.is_none() {
      if let Ok(file) = self.request(json!({"@type":"getFile","file_id":file_id}), Duration::from_secs(10)).await {
        local_path = local_path_from_file(&file);
      }
    }

    let Some(src) = local_path else {
      return Err(TgError::Other("Не удалось получить локальный путь к файлу".into()));
    };
    let src_path = PathBuf::from(src);
    if !src_path.exists() {
      return Err(TgError::Other("Файл не найден в кеше TDLib".into()));
    }

    if let Some(parent) = target.parent() {
      std::fs::create_dir_all(parent).map_err(TgError::Io)?;
    }
    std::fs::copy(&src_path, &target).map_err(TgError::Io)?;
    Ok(target)
  }

  async fn message_exists(&self, chat_id: ChatId, message_id: MessageId) -> Result<bool, TgError> {
    self.ensure_authorized().await?;
    let res = self
      .request(
        json!({
          "@type":"getMessage",
          "chat_id": chat_id,
          "message_id": message_id
        }),
        Duration::from_secs(10)
      )
      .await;

    match res {
      Ok(v) => {
        let id = v.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
        Ok(id == message_id)
      }
      Err(TgError::Other(msg)) => {
        let lowered = msg.to_lowercase();
        if lowered.contains("not found") || lowered.contains("message not found") {
          Ok(false)
        } else {
          Err(TgError::Other(msg))
        }
      }
      Err(e) => Err(e)
    }
  }
}

struct CommandCtx<'a> {
  paths: &'a Paths,
  config: &'a mut Option<TdlibConfig>,
  lib_path: &'a mut Option<PathBuf>,
  client: &'a mut Option<TdlibClient>,
  waiting_for_params: &'a mut bool,
  params_sent: &'a mut bool,
  pending_requests: &'a mut PendingRequests,
  next_request_id: &'a mut u64,
  build_attempted: &'a mut bool,
  pending: &'a mut Vec<String>,
  app: &'a tauri::AppHandle,
  last_state: &'a mut Option<AuthState>
}

fn handle_command(cmd: TdlibCommand, ctx: &mut CommandCtx<'_>) {
  match cmd {
    TdlibCommand::Td(m) => {
      if let Some(c) = ctx.client.as_ref() {
        let _ = c.send(&m);
      } else {
        ctx.pending.push(m);
      }
    }
    TdlibCommand::SetConfig { api_id, api_hash, tdlib_path } => {
      let session_name = tdlib_session_name();
      match TdlibConfig::from_settings(ctx.paths, api_id, api_hash, &session_name) {
        Ok(cfg) => {
          *ctx.config = Some(cfg);
        }
        Err(e) => {
          tracing::error!("Не удалось применить настройки TDLib: {e}");
          return;
        }
      }

      *ctx.lib_path = resolve_tdlib_path(ctx.paths, tdlib_path.as_deref());
      *ctx.build_attempted = false;

      if ctx.client.is_some() && *ctx.waiting_for_params {
        if let Some(cfg) = ctx.config.as_ref() {
          if !*ctx.params_sent {
            let payload = build_tdlib_parameters(cfg);
            if let Some(c) = ctx.client.as_ref() {
              let _ = c.send(&payload);
            }
            *ctx.params_sent = true;
            *ctx.waiting_for_params = false;
          }
        }
      }

      if ctx.client.is_none() && (ctx.config.is_none() || ctx.lib_path.is_none()) {
        set_auth_state(ctx.app, AuthState::WaitConfig, ctx.last_state);
      }
    }
    TdlibCommand::Request { payload, respond_to } => {
      if ctx.client.is_none() {
        let _ = respond_to.send(Err(anyhow::anyhow!("TDLib еще не инициализирован")));
        return;
      }

      let mut request = payload;
      let Some(obj) = request.as_object_mut() else {
        let _ = respond_to.send(Err(anyhow::anyhow!("Запрос к TDLib должен быть объектом")));
        return;
      };

      let request_id = *ctx.next_request_id;
      *ctx.next_request_id = (*ctx.next_request_id).wrapping_add(1).max(1);
      obj.insert("@extra".to_string(), json!(request_id));

      ctx.pending_requests.insert(request_id, respond_to);
      if let Some(c) = ctx.client.as_ref() {
        let _ = c.send(&request.to_string());
      }
    }
  }
}

fn attempt_tdlib_build(paths: &Paths, app: &tauri::AppHandle) -> anyhow::Result<PathBuf> {
  emit_build(app, "start", "Начинаю сборку TDLib", None);
  let base = tdlib_reserved_dir(paths);
  std::fs::create_dir_all(&base)?;

  let repo_dir = base.join("td");
  if !repo_dir.exists() {
    emit_build(app, "clone", "Клонирую репозиторий TDLib", None);
    let mut cmd = Command::new("git");
    cmd.arg("clone")
      .arg("--depth")
      .arg("1")
      .arg("https://github.com/tdlib/td.git")
      .arg(&repo_dir);
    if let Err(e) = run_command(cmd, "git clone", app) {
      emit_build(app, "error", "Не удалось скачать TDLib", Some(e.to_string()));
      return Err(e);
    }
  }

  let build_dir = repo_dir.join("build");
  std::fs::create_dir_all(&build_dir)?;

  let mut need_configure = !build_dir.join("CMakeCache.txt").exists();
  if !build_system_ready(&build_dir) {
    need_configure = true;
  }

  if need_configure {
    emit_build(app, "configure", "Готовлю сборку (cmake)", None);
    if build_dir.exists() {
      let _ = std::fs::remove_dir_all(&build_dir);
    }
    std::fs::create_dir_all(&build_dir)?;
    let mut cmd = Command::new("cmake");
    cmd.arg("-DCMAKE_BUILD_TYPE=Release")
      .arg("..")
      .current_dir(&build_dir);
    if let Err(e) = run_command(cmd, "cmake configure", app) {
      emit_build(app, "error", "Ошибка конфигурации CMake", Some(e.to_string()));
      return Err(e);
    }
    if !build_system_ready(&build_dir) {
      let msg = "CMake не создал файлы сборки (Makefile/Ninja)";
      emit_build(app, "error", msg, Some(msg.to_string()));
      return Err(anyhow::anyhow!(msg));
    }
  }

  emit_build(app, "build", "Собираю TDLib", None);
  let mut build_cmd = Command::new("cmake");
  build_cmd.arg("--build").arg(".").arg("--target").arg("tdjson");
  #[cfg(target_os = "windows")]
  {
    build_cmd.arg("--config").arg("Release");
  }
  build_cmd.current_dir(&build_dir);

  if let Err(e) = run_command(build_cmd, "cmake build", app) {
    emit_build(app, "error", "Ошибка сборки TDLib", Some(e.to_string()));
    return Err(e);
  }

  if let Some(p) = find_tdjson_lib(&repo_dir) {
    emit_build(app, "success", "TDLib собран", Some(p.to_string_lossy().to_string()));
    Ok(p)
  } else {
    let msg = "Сборка прошла, но libtdjson не найден";
    emit_build(app, "error", msg, Some(msg.to_string()));
    Err(anyhow::anyhow!(msg))
  }
}

fn run_command(mut cmd: Command, name: &str, app: &tauri::AppHandle) -> anyhow::Result<()> {
  cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
  let mut child = cmd.spawn()?;

  let stdout = child.stdout.take();
  let stderr = child.stderr.take();

  let (tx, rx) = mpsc::channel::<(String, String)>();

  if let Some(out) = stdout {
    let tx = tx.clone();
    std::thread::spawn(move || {
      let reader = BufReader::new(out);
      for line in reader.lines().map_while(Result::ok) {
        let _ = tx.send(("stdout".to_string(), line));
      }
    });
  }

  if let Some(err) = stderr {
    let tx = tx.clone();
    std::thread::spawn(move || {
      let reader = BufReader::new(err);
      for line in reader.lines().map_while(Result::ok) {
        let _ = tx.send(("stderr".to_string(), line));
      }
    });
  }

  drop(tx);

  let mut stdout_buf = String::new();
  let mut stderr_buf = String::new();

  for (stream, line) in rx {
    emit_build_log(app, &stream, &line);
    if stream == "stdout" {
      stdout_buf.push_str(&line);
      stdout_buf.push('\n');
    } else {
      stderr_buf.push_str(&line);
      stderr_buf.push('\n');
    }
  }

  let status = child.wait()?;
  if !status.success() {
    return Err(anyhow::anyhow!(
      "{name} завершился с ошибкой. stdout: {stdout_buf} stderr: {stderr_buf}"
    ));
  }
  Ok(())
}

fn build_system_ready(build_dir: &Path) -> bool {
  build_dir.join("Makefile").exists() || build_dir.join("build.ninja").exists()
}

#[derive(Deserialize)]
struct TdlibManifest {
  version: String,
  assets: Vec<TdlibManifestAsset>
}

#[derive(Deserialize, Clone)]
struct TdlibManifestAsset {
  platform: String,
  file: String,
  url: String,
  sha256: Option<String>,
  size: Option<u64>,
  tdlib_commit: Option<String>
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

fn tdlib_platform_id() -> Option<String> {
  let os = match std::env::consts::OS {
    "windows" => "windows",
    "macos" => "macos",
    "linux" => "linux",
    _ => return None
  };
  let arch = match std::env::consts::ARCH {
    "x86_64" => "x86_64",
    "aarch64" => "aarch64",
    "amd64" => "x86_64",
    _ => return None
  };
  Some(format!("{os}-{arch}"))
}

fn tdlib_prebuilt_dir(paths: &Paths) -> PathBuf {
  tdlib_reserved_dir(paths).join("prebuilt")
}

fn tdlib_prebuilt_platform_dir(paths: &Paths) -> Option<PathBuf> {
  tdlib_platform_id().map(|platform| tdlib_prebuilt_dir(paths).join(platform))
}

fn tdlib_repo() -> Option<String> {
  if let Ok(raw) = std::env::var("CLOUDTG_TDLIB_REPO") {
    if let Some(repo) = extract_github_repo(&raw) {
      return Some(repo);
    }
  }
  if let Some(raw) = option_env!("CLOUDTG_TDLIB_REPO") {
    if let Some(repo) = extract_github_repo(raw) {
      return Some(repo);
    }
  }
  if let Some(raw) = option_env!("CARGO_PKG_REPOSITORY") {
    if let Some(repo) = extract_github_repo(raw) {
      return Some(repo);
    }
  }
  None
}

fn extract_github_repo(raw: &str) -> Option<String> {
  let trimmed = raw.trim();
  if trimmed.is_empty() {
    return None;
  }
  if trimmed.contains('/') && !trimmed.contains("github.com") && !trimmed.contains(':') {
    return Some(trimmed.trim_end_matches(".git").to_string());
  }
  if let Some(pos) = trimmed.find("github.com") {
    let mut s = &trimmed[pos + "github.com".len()..];
    s = s.trim_start_matches(&[':', '/'][..]);
    let s = s.trim_end_matches(".git");
    if let Some((owner, repo)) = s.split_once('/') {
      if !owner.is_empty() && !repo.is_empty() {
        return Some(format!("{owner}/{repo}"));
      }
    }
  }
  None
}

fn github_token() -> Option<String> {
  std::env::var("GITHUB_TOKEN")
    .ok()
    .or_else(|| std::env::var("GH_TOKEN").ok())
}

fn http_agent() -> ureq::Agent {
  ureq::Agent::config_builder()
    .timeout_connect(Some(Duration::from_secs(10)))
    .timeout_recv_body(Some(Duration::from_secs(60)))
    .build()
    .into()
}

fn find_tdjson_lib(root: &Path) -> Option<PathBuf> {
  let mut stack = vec![root.to_path_buf()];
  let exact_names = [
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
        if exact_names.iter().any(|n| n == &name)
          || name.starts_with("libtdjson.so")
          || name.starts_with("libtdjson.dylib")
        {
          return Some(path);
        }
      }
    }
  }
  None
}

fn resolve_tdlib_manifest_url(repo: &str) -> anyhow::Result<Option<String>> {
  if let Ok(url) = std::env::var("CLOUDTG_TDLIB_MANIFEST_URL") {
    if !url.trim().is_empty() {
      return Ok(Some(url));
    }
  }

  let agent = http_agent();
  let mut req = agent
    .get(&format!("https://api.github.com/repos/{repo}/releases/latest"))
    .header("User-Agent", "cloudtg")
    .header("Accept", "application/vnd.github+json");
  if let Some(token) = github_token() {
    req = req.header("Authorization", &format!("Bearer {token}"));
  }
  let response = req.call().map_err(|e| anyhow::anyhow!("Не удалось получить релиз TDLib: {e}"))?;
  let body = response.into_body().read_to_string().map_err(|e| anyhow::anyhow!("Не удалось прочитать ответ релиза: {e}"))?;
  let json: Value = serde_json::from_str(&body)?;
  if let Some(url) = find_manifest_url(&json) {
    return Ok(Some(url));
  }
  let tag = json.get("tag_name").and_then(|v| v.as_str()).unwrap_or("");
  tracing::info!(event = "tdlib_manifest_missing", tag = tag, "Манифест TDLib не найден в latest релизе");

  let mut req = agent
    .get(&format!("https://api.github.com/repos/{repo}/releases?per_page=10"))
    .header("User-Agent", "cloudtg")
    .header("Accept", "application/vnd.github+json");
  if let Some(token) = github_token() {
    req = req.header("Authorization", &format!("Bearer {token}"));
  }
  let response = req.call().map_err(|e| anyhow::anyhow!("Не удалось получить список релизов TDLib: {e}"))?;
  let body = response.into_body().read_to_string().map_err(|e| anyhow::anyhow!("Не удалось прочитать список релизов: {e}"))?;
  let releases: Value = serde_json::from_str(&body)?;
  let Some(list) = releases.as_array() else {
    return Ok(None);
  };
  for rel in list {
    if let Some(url) = find_manifest_url(rel) {
      let tag = rel.get("tag_name").and_then(|v| v.as_str()).unwrap_or("");
      tracing::info!(event = "tdlib_manifest_found", tag = tag, "Найден манифест TDLib в релизе");
      return Ok(Some(url));
    }
  }

  Ok(None)
}

fn find_manifest_url(release: &Value) -> Option<String> {
  let assets = release.get("assets").and_then(|v| v.as_array())?;
  for asset in assets {
    let name = asset.get("name").and_then(|v| v.as_str()).unwrap_or("");
    if name == TDLIB_MANIFEST_NAME {
      let url = asset.get("browser_download_url").and_then(|v| v.as_str()).unwrap_or("");
      if !url.is_empty() {
        return Some(url.to_string());
      }
    }
  }
  None
}

fn fetch_tdlib_manifest(url: &str) -> anyhow::Result<TdlibManifest> {
  let agent = http_agent();
  let mut req = agent.get(url).header("User-Agent", "cloudtg");
  if let Some(token) = github_token() {
    req = req.header("Authorization", &format!("Bearer {token}"));
  }
  let response = req.call().map_err(|e| anyhow::anyhow!("Не удалось скачать манифест TDLib: {e}"))?;
  let body = response.into_body().read_to_string().map_err(|e| anyhow::anyhow!("Не удалось прочитать манифест: {e}"))?;
  let manifest: TdlibManifest = serde_json::from_str(&body)?;
  Ok(manifest)
}

fn download_tdlib_asset(
  url: &str,
  expected_sha256: Option<&str>,
  total_hint: Option<u64>,
  app: &tauri::AppHandle
) -> anyhow::Result<NamedTempFile> {
  let agent = http_agent();
  let mut req = agent.get(url).header("User-Agent", "cloudtg");
  if let Some(token) = github_token() {
    req = req.header("Authorization", &format!("Bearer {token}"));
  }
  let response = req.call().map_err(|e| anyhow::anyhow!("Не удалось скачать TDLib: {e}"))?;
  let mut total = response
    .headers()
    .get("Content-Length")
    .and_then(|v| v.to_str().ok())
    .and_then(|v| v.parse::<u64>().ok());
  if total.is_none() {
    total = total_hint;
  }
  let mut reader = response.into_body().into_reader();
  let mut tmp = NamedTempFile::new()?;
  let mut hasher = Sha256::new();
  let mut buf = [0u8; 8192];
  let mut downloaded: u64 = 0;
  let mut last_percent: i32 = -1;

  loop {
    let n = reader.read(&mut buf)?;
    if n == 0 {
      break;
    }
    tmp.write_all(&buf[..n])?;
    hasher.update(&buf[..n]);
    downloaded += n as u64;
    if let Some(total) = total {
      let percent = ((downloaded * 100) / total) as i32;
      if percent != last_percent && (0..=100).contains(&percent) {
        last_percent = percent;
        emit_build_log(app, "stdout", &format!("{percent}%"));
      }
    }
  }

  if let Some(expected) = expected_sha256 {
    let digest = hex::encode(hasher.finalize());
    if digest.to_lowercase() != expected.trim().to_lowercase() {
      return Err(anyhow::anyhow!("Checksum TDLib не совпадает"));
    }
  }

  Ok(tmp)
}

fn extract_tdlib_archive(archive: &Path, file_name: &str, dest: &Path) -> anyhow::Result<()> {
  if dest.exists() {
    std::fs::remove_dir_all(dest)?;
  }
  std::fs::create_dir_all(dest)?;

  if file_name.ends_with(".zip") {
    let file = std::fs::File::open(archive)?;
    let mut zip = ZipArchive::new(file)?;
    for i in 0..zip.len() {
      let mut entry = zip.by_index(i)?;
      let outpath = dest.join(entry.mangled_name());
      if entry.is_dir() {
        std::fs::create_dir_all(&outpath)?;
      } else {
        if let Some(parent) = outpath.parent() {
          std::fs::create_dir_all(parent)?;
        }
        let mut outfile = std::fs::File::create(&outpath)?;
        std::io::copy(&mut entry, &mut outfile)?;
      }
    }
    return Ok(());
  }

  if file_name.ends_with(".tar.gz") || file_name.ends_with(".tgz") {
    let file = std::fs::File::open(archive)?;
    let decoder = GzDecoder::new(file);
    let mut archive = Archive::new(decoder);
    for entry in archive.entries()? {
      let mut entry = entry?;
      let path = entry.path()?;
      let outpath = safe_join(dest, &path)?;
      if let Some(parent) = outpath.parent() {
        std::fs::create_dir_all(parent)?;
      }
      entry.unpack(&outpath)?;
    }
    return Ok(());
  }

  Err(anyhow::anyhow!("Неизвестный формат архива TDLib"))
}

fn safe_join(base: &Path, path: &Path) -> anyhow::Result<PathBuf> {
  let mut out = base.to_path_buf();
  for component in path.components() {
    match component {
      std::path::Component::Normal(p) => out.push(p),
      std::path::Component::CurDir => {}
      _ => return Err(anyhow::anyhow!("Некорректный путь в архиве TDLib"))
    }
  }
  Ok(out)
}

fn attempt_tdlib_download(paths: &Paths, app: &tauri::AppHandle) -> anyhow::Result<Option<PathBuf>> {
  let Some(platform) = tdlib_platform_id() else {
    tracing::info!("Платформа не поддерживается для предсобранной TDLib");
    return Ok(None);
  };
  let repo = tdlib_repo();
  let Some(repo) = repo.as_ref() else {
    tracing::info!("Репозиторий TDLib не задан, пропускаю автозагрузку");
    return Ok(None);
  };

  let Some(manifest_url) = resolve_tdlib_manifest_url(repo)? else {
    tracing::info!("Манифест TDLib не найден, пропускаю автозагрузку");
    return Ok(None);
  };
  emit_build(app, "download", "Скачиваю предсобранную TDLib", None);
  let manifest = fetch_tdlib_manifest(&manifest_url)?;
  tracing::info!(
    event = "tdlib_manifest_loaded",
    version = %manifest.version,
    assets = manifest.assets.len(),
    "Манифест TDLib загружен"
  );
  let asset = manifest.assets.iter().find(|a| a.platform == platform);
  let Some(asset) = asset else {
    return Ok(None);
  };

  tracing::info!(
    event = "tdlib_manifest_asset",
    platform = %asset.platform,
    size = asset.size.unwrap_or(0),
    tdlib_commit = asset.tdlib_commit.as_deref().unwrap_or(""),
    "Выбран артефакт TDLib"
  );

  let tmp = download_tdlib_asset(&asset.url, asset.sha256.as_deref(), asset.size, app)?;
  let Some(dest) = tdlib_prebuilt_platform_dir(paths) else {
    return Ok(None);
  };
  extract_tdlib_archive(tmp.path(), &asset.file, &dest)?;
  if let Some(lib) = find_tdjson_lib(&dest) {
    emit_build(app, "success", "TDLib скачан", Some(lib.to_string_lossy().to_string()));
    return Ok(Some(lib));
  }
  Err(anyhow::anyhow!("Не удалось найти библиотеку TDLib после распаковки"))
}

fn resolve_tdlib_path(paths: &Paths, configured: Option<&str>) -> Option<PathBuf> {
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

  if let Some(prebuilt_dir) = tdlib_prebuilt_platform_dir(paths) {
    if let Some(p) = find_tdjson_lib(&prebuilt_dir) {
      return Some(p);
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

struct ResponseCtx<'a> {
  client: &'a TdlibClient,
  config: &'a mut Option<TdlibConfig>,
  waiting_for_params: &'a mut bool,
  params_sent: &'a mut bool,
  app: &'a tauri::AppHandle,
  last_state: &'a mut Option<AuthState>,
  send_waiters: &'a SendWaiters,
  send_results: &'a SendResults
}

fn handle_tdlib_response(v: &Value, ctx: &mut ResponseCtx<'_>) -> anyhow::Result<()> {
  let t = v.get("@type").and_then(|v| v.as_str()).unwrap_or("");

  if t == "updateAuthorizationState" {
    if let Some(state) = v.get("authorization_state") {
      handle_auth_state(
        state,
        ctx.client,
        ctx.config,
        ctx.waiting_for_params,
        ctx.params_sent,
        ctx.app,
        ctx.last_state
      )?;
    }
    return Ok(());
  }

  if t.starts_with("authorizationState") {
    handle_auth_state(
      v,
      ctx.client,
      ctx.config,
      ctx.waiting_for_params,
      ctx.params_sent,
      ctx.app,
      ctx.last_state
    )?;
    return Ok(());
  }

  if t == "updateNewMessage" {
    if let Some(message) = v.get("message") {
      if let Some((chat_id, msg)) = history_message_from_object(message) {
        schedule_storage_index(ctx.app, chat_id, msg);
      }
    }
    return Ok(());
  }

  if t == "updateMessageContent" {
    let chat_id = v.get("chat_id").and_then(|v| v.as_i64()).unwrap_or(0);
    let message_id = v.get("message_id").and_then(|v| v.as_i64()).unwrap_or(0);
    if chat_id != 0 && message_id != 0 {
      if let Some(content) = v.get("new_content") {
        let msg = history_message_from_content(message_id, Utc::now().timestamp(), content);
        schedule_storage_index(ctx.app, chat_id, msg);
      }
    }
    return Ok(());
  }

  if t == "updateMessageSendSucceeded" {
    if let Some(old_id) = v.get("old_message_id").and_then(|v| v.as_i64()) {
      let new_id = v
        .get("message")
        .and_then(|m| m.get("id"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
      if let Some(tx) = ctx.send_waiters.lock().remove(&old_id) {
        let _ = tx.send(Ok(new_id));
      } else {
        let mut guard = ctx.send_results.lock();
        guard.insert(old_id, Ok(new_id));
        if guard.len() > 128 {
          guard.clear();
        }
      }
    }
    return Ok(());
  }

  if t == "updateMessageSendFailed" {
    if let Some(old_id) = v.get("old_message_id").and_then(|v| v.as_i64()) {
      let err = v
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str())
        .unwrap_or("Не удалось отправить сообщение")
        .to_string();
      if let Some(tx) = ctx.send_waiters.lock().remove(&old_id) {
        let _ = tx.send(Err(anyhow::anyhow!(err.clone())));
      } else {
        let mut guard = ctx.send_results.lock();
        guard.insert(old_id, Err(err));
        if guard.len() > 128 {
          guard.clear();
        }
      }
    }
    return Ok(());
  }

  if t == "error" {
    let msg = v.get("message").and_then(|m| m.as_str()).unwrap_or("неизвестная ошибка");
    tracing::error!("TDLib вернул ошибку: {msg}");
  }

  Ok(())
}

fn handle_request_response(
  v: &Value,
  pending_requests: &mut HashMap<u64, oneshot::Sender<anyhow::Result<Value>>>
) -> bool {
  let Some(extra) = v.get("@extra") else {
    return false;
  };
  let id = match extra {
    Value::Number(n) => n.as_u64(),
    Value::String(s) => s.parse::<u64>().ok(),
    _ => None
  };
  let Some(id) = id else {
    return false;
  };
  let Some(tx) = pending_requests.remove(&id) else {
    return false;
  };

  if v.get("@type").and_then(|t| t.as_str()) == Some("error") {
    let msg = v
      .get("message")
      .and_then(|m| m.as_str())
      .unwrap_or("неизвестная ошибка")
      .to_string();
    let _ = tx.send(Err(anyhow::anyhow!(msg)));
  } else {
    let _ = tx.send(Ok(v.clone()));
  }
  true
}

fn handle_auth_state(
  state: &serde_json::Value,
  client: &TdlibClient,
  config: &mut Option<TdlibConfig>,
  waiting_for_params: &mut bool,
  params_sent: &mut bool,
  app: &tauri::AppHandle,
  last_state: &mut Option<AuthState>
) -> anyhow::Result<()> {
  let t = state.get("@type").and_then(|v| v.as_str()).unwrap_or("");

  match t {
    "authorizationStateWaitTdlibParameters" => {
      if *params_sent {
        tracing::debug!("TDLib уже получил параметры, пропускаю повторную отправку");
        return Ok(());
      }
      if let Some(cfg) = config.as_ref() {
        let payload = build_tdlib_parameters(cfg);
        let _ = client.send(&payload);
        *params_sent = true;
        *waiting_for_params = false;
        set_auth_state(app, AuthState::Unknown, last_state);
      } else {
        *waiting_for_params = true;
        set_auth_state(app, AuthState::WaitConfig, last_state);
      }
    }
    "authorizationStateWaitEncryptionKey" => {
      let payload = json!({"@type":"checkDatabaseEncryptionKey","encryption_key":""}).to_string();
      let _ = client.send(&payload);
      set_auth_state(app, AuthState::Unknown, last_state);
    }
    "authorizationStateWaitPhoneNumber" => {
      set_auth_state(app, AuthState::WaitPhone, last_state);
    }
    "authorizationStateWaitCode" => {
      set_auth_state(app, AuthState::WaitCode, last_state);
    }
    "authorizationStateWaitPassword" => {
      set_auth_state(app, AuthState::WaitPassword, last_state);
    }
    "authorizationStateReady" => {
      set_auth_state(app, AuthState::Ready, last_state);
    }
    "authorizationStateClosing" | "authorizationStateLoggingOut" => {
      *params_sent = false;
      set_auth_state(app, AuthState::Unknown, last_state);
    }
    "authorizationStateClosed" => {
      *params_sent = false;
      set_auth_state(app, AuthState::Closed, last_state);
    }
    "authorizationStateWaitRegistration" => {
      tracing::warn!("Требуется регистрация аккаунта, это пока не поддержано");
      set_auth_state(app, AuthState::Unknown, last_state);
    }
    _ => {
      tracing::debug!("Неизвестное состояние авторизации: {t}");
    }
  }

  Ok(())
}

fn set_auth_state(app: &tauri::AppHandle, state: AuthState, last_state: &mut Option<AuthState>) {
  if last_state.as_ref() == Some(&state) {
    return;
  }

  let app_state = app.state::<AppState>();
  app_state.set_auth_state(state.clone());
  *last_state = Some(state.clone());

  let payload = AuthEvent { state: auth_state_to_str(&state).to_string() };
  let _ = app.emit("auth_state_changed", payload);
}

#[derive(Clone, serde::Serialize)]
struct AuthEvent {
  state: String
}

#[derive(Clone, serde::Serialize)]
struct BuildEvent {
  state: String,
  message: String,
  detail: Option<String>
}

fn auth_state_to_str(state: &AuthState) -> &'static str {
  match state {
    AuthState::Unknown => "unknown",
    AuthState::WaitConfig => "wait_config",
    AuthState::WaitPhone => "wait_phone",
    AuthState::WaitCode => "wait_code",
    AuthState::WaitPassword => "wait_password",
    AuthState::Ready => "ready",
    AuthState::Closed => "closed"
  }
}

fn emit_build(app: &tauri::AppHandle, state: &str, message: &str, detail: Option<String>) {
  let detail_for_log = detail.clone();
  let payload = BuildEvent {
    state: state.to_string(),
    message: message.to_string(),
    detail
  };
  let _ = app.emit("tdlib_build_status", payload);
  tracing::info!(
    event = "tdlib_build_status",
    state = state,
    message = message,
    detail = detail_for_log.as_deref().unwrap_or(""),
    "TDLib"
  );
}

fn emit_build_log(app: &tauri::AppHandle, stream: &str, line: &str) {
  let payload = BuildLogEvent {
    stream: stream.to_string(),
    line: line.to_string()
  };
  let _ = app.emit("tdlib_build_log", payload);
  tracing::debug!(event = "tdlib_build_log", stream = stream, line = line, "TDLib");
}

#[derive(Clone, serde::Serialize)]
struct BuildLogEvent {
  stream: String,
  line: String
}

fn build_tdlib_parameters(config: &TdlibConfig) -> String {
  json!({
    "@type": "setTdlibParameters",
    "use_test_dc": false,
    "database_directory": path_to_str(&config.db_dir),
    "files_directory": path_to_str(&config.files_dir),
    "use_file_database": true,
    "use_chat_info_database": true,
    "use_message_database": true,
    "use_secret_chats": false,
    "api_id": config.api_id,
    "api_hash": config.api_hash.clone(),
    "system_language_code": "ru",
    "device_model": "CloudTG",
    "system_version": std::env::consts::OS,
    "application_version": env!("CARGO_PKG_VERSION"),
    "enable_storage_optimizer": true,
    "ignore_file_names": false
  })
  .to_string()
}

fn path_to_str(p: &Path) -> String {
  p.to_string_lossy().to_string()
}
