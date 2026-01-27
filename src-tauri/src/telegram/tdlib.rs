use std::{
  ffi::{CStr, CString},
  os::raw::{c_char, c_double, c_int, c_void},
  path::{Path, PathBuf},
  sync::mpsc,
  time::Duration
};

use libloading::Library;
use serde_json::json;
use tauri::{Emitter, Manager};

use crate::paths::Paths;
use crate::state::{AppState, AuthState};
use super::{ChatId, MessageId, TelegramService, TgError, UploadedMessage};

struct TdlibConfig {
  api_id: i32,
  api_hash: String,
  db_dir: PathBuf,
  files_dir: PathBuf
}

impl TdlibConfig {
  fn load(paths: &Paths) -> anyhow::Result<Self> {
    let api_id = std::env::var("CLOUDTG_TG_API_ID")
      .map_err(|_| anyhow::anyhow!("Не задана переменная окружения CLOUDTG_TG_API_ID"))?
      .parse::<i32>()
      .map_err(|_| anyhow::anyhow!("CLOUDTG_TG_API_ID должен быть числом"))?;
    let api_hash = std::env::var("CLOUDTG_TG_API_HASH")
      .map_err(|_| anyhow::anyhow!("Не задана переменная окружения CLOUDTG_TG_API_HASH"))?;

    let db_dir = paths.data_dir.join("tdlib");
    let files_dir = paths.cache_dir.join("tdlib_files");
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
    unsafe {
      let lib = Library::new(path)?;
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
  tx: mpsc::Sender<String>
}

impl TdlibTelegram {
  pub fn new(paths: Paths, app: tauri::AppHandle) -> anyhow::Result<Self> {
    let config = TdlibConfig::load(&paths)?;
    let lib_path = resolve_tdlib_path(&paths)?;

    let (tx, rx) = mpsc::channel::<String>();

    let client = TdlibClient::load(&lib_path)?;
    client.set_verbosity(2);

    let app_for_thread = app.clone();

    std::thread::spawn(move || {
      let mut last_state: Option<AuthState> = None;

      let _ = client.send(&json!({"@type":"getAuthorizationState"}).to_string());

      loop {
        match rx.recv_timeout(Duration::from_millis(10)) {
          Ok(msg) => {
            let _ = client.send(&msg);
          }
          Err(mpsc::RecvTimeoutError::Timeout) => {}
          Err(mpsc::RecvTimeoutError::Disconnected) => break
        }

        while let Ok(msg) = rx.try_recv() {
          let _ = client.send(&msg);
        }

        if let Some(resp) = client.receive(0.1) {
          if let Err(e) = handle_tdlib_response(&resp, &client, &config, &app_for_thread, &mut last_state) {
            tracing::error!("Ошибка TDLib: {e}");
          }
        }
      }

      client.destroy();
    });

    Ok(Self { tx })
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
      .send(payload)
      .map_err(|_| TgError::Other("TDLib поток не запущен".into()))?;
    Ok(())
  }

  async fn auth_submit_code(&self, code: String) -> Result<(), TgError> {
    let payload = json!({"@type":"checkAuthenticationCode","code":code}).to_string();
    self.tx
      .send(payload)
      .map_err(|_| TgError::Other("TDLib поток не запущен".into()))?;
    Ok(())
  }

  async fn auth_submit_password(&self, password: String) -> Result<(), TgError> {
    let payload = json!({"@type":"checkAuthenticationPassword","password":password}).to_string();
    self.tx
      .send(payload)
      .map_err(|_| TgError::Other("TDLib поток не запущен".into()))?;
    Ok(())
  }

  async fn storage_get_or_create_channel(&self) -> Result<ChatId, TgError> {
    Err(TgError::NotImplemented)
  }

  async fn send_dir_message(&self, _chat_id: ChatId, _text: String) -> Result<UploadedMessage, TgError> {
    Err(TgError::NotImplemented)
  }

  async fn send_file(&self, _chat_id: ChatId, _path: std::path::PathBuf, _caption: String) -> Result<UploadedMessage, TgError> {
    Err(TgError::NotImplemented)
  }

  async fn download_message_file(&self, _chat_id: ChatId, _message_id: MessageId, _target: std::path::PathBuf)
    -> Result<std::path::PathBuf, TgError> {
    Err(TgError::NotImplemented)
  }
}

fn resolve_tdlib_path(paths: &Paths) -> anyhow::Result<PathBuf> {
  if let Ok(p) = std::env::var("CLOUDTG_TDLIB_PATH") {
    return Ok(PathBuf::from(p));
  }

  let base = &paths.base_dir;

  #[cfg(target_os = "windows")]
  let candidates = [base.join("tdjson.dll")];

  #[cfg(target_os = "macos")]
  let candidates = [base.join("libtdjson.dylib")];

  #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
  let candidates = [base.join("libtdjson.so"), base.join("libtdjson.so.1")];

  for c in candidates {
    if c.exists() {
      return Ok(c);
    }
  }

  Err(anyhow::anyhow!(
    "Не удалось найти libtdjson. Укажи CLOUDTG_TDLIB_PATH или положи библиотеку рядом с бинарём"
  ))
}

fn handle_tdlib_response(
  raw: &str,
  client: &TdlibClient,
  config: &TdlibConfig,
  app: &tauri::AppHandle,
  last_state: &mut Option<AuthState>
) -> anyhow::Result<()> {
  let v: serde_json::Value = serde_json::from_str(raw)?;
  let t = v.get("@type").and_then(|v| v.as_str()).unwrap_or("");

  if t == "updateAuthorizationState" {
    if let Some(state) = v.get("authorization_state") {
      handle_auth_state(state, client, config, app, last_state)?;
    }
    return Ok(());
  }

  if t.starts_with("authorizationState") {
    handle_auth_state(&v, client, config, app, last_state)?;
    return Ok(());
  }

  if t == "error" {
    let msg = v.get("message").and_then(|m| m.as_str()).unwrap_or("неизвестная ошибка");
    tracing::error!("TDLib вернул ошибку: {msg}");
  }

  Ok(())
}

fn handle_auth_state(
  state: &serde_json::Value,
  client: &TdlibClient,
  config: &TdlibConfig,
  app: &tauri::AppHandle,
  last_state: &mut Option<AuthState>
) -> anyhow::Result<()> {
  let t = state.get("@type").and_then(|v| v.as_str()).unwrap_or("");

  match t {
    "authorizationStateWaitTdlibParameters" => {
      let payload = json!({
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
      .to_string();
      let _ = client.send(&payload);
      set_auth_state(app, AuthState::Unknown, last_state);
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
      set_auth_state(app, AuthState::Unknown, last_state);
    }
    "authorizationStateClosed" => {
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

fn auth_state_to_str(state: &AuthState) -> &'static str {
  match state {
    AuthState::Unknown => "unknown",
    AuthState::WaitPhone => "wait_phone",
    AuthState::WaitCode => "wait_code",
    AuthState::WaitPassword => "wait_password",
    AuthState::Ready => "ready",
    AuthState::Closed => "closed"
  }
}

fn path_to_str(p: &Path) -> String {
  p.to_string_lossy().to_string()
}
