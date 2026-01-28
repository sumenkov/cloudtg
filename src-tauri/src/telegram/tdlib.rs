use std::{
  collections::HashMap,
  ffi::{CStr, CString},
  io::{BufRead, BufReader},
  os::raw::{c_char, c_double, c_int, c_void},
  path::{Path, PathBuf},
  process::{Command, Stdio},
  sync::mpsc,
  time::Duration
};

use libloading::Library;
use serde_json::{json, Value};
use tauri::{Emitter, Manager};
use tokio::sync::oneshot;

use crate::paths::Paths;
use crate::state::{AppState, AuthState};
use crate::settings::TgSettings;
use super::{ChatId, MessageId, TelegramService, TgError, UploadedMessage};

#[derive(Clone)]
struct TdlibConfig {
  api_id: i32,
  api_hash: String,
  db_dir: PathBuf,
  files_dir: PathBuf
}

impl TdlibConfig {
  fn from_settings(paths: &Paths, api_id: i32, api_hash: String) -> anyhow::Result<Self> {
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
  tx: mpsc::Sender<TdlibCommand>,
  paths: Paths
}

enum TdlibCommand {
  Td(String),
  SetConfig { api_id: i32, api_hash: String, tdlib_path: Option<String> },
  Request { payload: Value, respond_to: oneshot::Sender<anyhow::Result<Value>> }
}

const STORAGE_CHANNEL_TITLE: &str = "CloudTG";
const STORAGE_CHANNEL_TITLE_LEGACY: &str = "CloudVault";

fn storage_channel_title() -> &'static str {
  STORAGE_CHANNEL_TITLE
}

fn storage_channel_title_legacy() -> &'static str {
  STORAGE_CHANNEL_TITLE_LEGACY
}

fn app_icon_bytes() -> &'static [u8] {
  include_bytes!("../../icons/icon.png")
}

impl TdlibTelegram {
  pub fn new(
    paths: Paths,
    app: tauri::AppHandle,
    initial_settings: Option<TgSettings>,
    initial_tdlib_path: Option<String>
  ) -> anyhow::Result<Self> {
    let (tx, rx) = mpsc::channel::<TdlibCommand>();

    let app_for_thread = app.clone();
    let paths_for_thread = paths.clone();
    let mut config = match initial_settings {
      Some(s) => Some(TdlibConfig::from_settings(&paths, s.api_id, s.api_hash)?),
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
      let mut pending_requests: HashMap<u64, oneshot::Sender<anyhow::Result<Value>>> = HashMap::new();
      let mut next_request_id: u64 = 1;

      if config.is_none() || lib_path.is_none() {
        set_auth_state(&app_for_thread, AuthState::WaitConfig, &mut last_state);
      }

      loop {
        match rx.recv_timeout(Duration::from_millis(10)) {
          Ok(cmd) => {
            handle_command(
              cmd,
              &paths_for_thread,
              &mut config,
              &mut lib_path,
              &mut client,
              &mut waiting_for_params,
              &mut params_sent,
              &mut pending_requests,
              &mut next_request_id,
              &mut build_attempted,
              &mut pending,
              &app_for_thread,
              &mut last_state
            );
          }
          Err(mpsc::RecvTimeoutError::Timeout) => {}
          Err(mpsc::RecvTimeoutError::Disconnected) => break
        }

        while let Ok(cmd) = rx.try_recv() {
          handle_command(
            cmd,
            &paths_for_thread,
            &mut config,
            &mut lib_path,
            &mut client,
            &mut waiting_for_params,
            &mut params_sent,
            &mut pending_requests,
            &mut next_request_id,
            &mut build_attempted,
            &mut pending,
            &app_for_thread,
            &mut last_state
          );
        }

        if client.is_none() {
          if config.is_some() && lib_path.is_none() && !build_attempted {
            build_attempted = true;
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
                tracing::error!("Не удалось загрузить TDLib: {e}");
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
            if let Err(e) = handle_tdlib_response(
              &value,
              c,
              &mut config,
              &mut waiting_for_params,
              &mut params_sent,
              &app_for_thread,
              &mut last_state
            ) {
              tracing::error!("Ошибка TDLib: {e}");
            }
          }
        }
      }

      if let Some(c) = client {
        c.destroy();
      }
    });

    Ok(Self { tx, paths })
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

  fn ensure_icon_file(&self) -> Result<PathBuf, TgError> {
    let icon_path = self.paths.cache_dir.join("cloudtg-icon.png");
    let bytes = app_icon_bytes();
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

  async fn storage_create_channel(&self) -> Result<ChatId, TgError> {
    self.ensure_authorized().await?;
    tracing::info!(event = "storage_channel_create_manual", "Создаю новый канал хранения по запросу");
    let chat_id = self.create_storage_channel().await?;
    if let Err(e) = self.ensure_storage_channel_config(chat_id).await {
      tracing::warn!(event = "storage_channel_config_failed", chat_id = chat_id, error = %e, "Не удалось обновить настройки канала");
    }
    Ok(chat_id)
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

  async fn send_file(&self, _chat_id: ChatId, _path: std::path::PathBuf, _caption: String) -> Result<UploadedMessage, TgError> {
    Err(TgError::NotImplemented)
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

  async fn download_message_file(&self, _chat_id: ChatId, _message_id: MessageId, _target: std::path::PathBuf)
    -> Result<std::path::PathBuf, TgError> {
    Err(TgError::NotImplemented)
  }
}

fn handle_command(
  cmd: TdlibCommand,
  paths: &Paths,
  config: &mut Option<TdlibConfig>,
  lib_path: &mut Option<PathBuf>,
  client: &mut Option<TdlibClient>,
  waiting_for_params: &mut bool,
  params_sent: &mut bool,
  pending_requests: &mut HashMap<u64, oneshot::Sender<anyhow::Result<Value>>>,
  next_request_id: &mut u64,
  build_attempted: &mut bool,
  pending: &mut Vec<String>,
  app: &tauri::AppHandle,
  last_state: &mut Option<AuthState>
) {
  match cmd {
    TdlibCommand::Td(m) => {
      if let Some(c) = client.as_ref() {
        let _ = c.send(&m);
      } else {
        pending.push(m);
      }
    }
    TdlibCommand::SetConfig { api_id, api_hash, tdlib_path } => {
      match TdlibConfig::from_settings(paths, api_id, api_hash) {
        Ok(cfg) => {
          *config = Some(cfg);
        }
        Err(e) => {
          tracing::error!("Не удалось применить настройки TDLib: {e}");
          return;
        }
      }

      *lib_path = resolve_tdlib_path(paths, tdlib_path.as_deref());
      *build_attempted = false;

      if client.is_some() && *waiting_for_params {
        if let Some(cfg) = config.as_ref() {
          if !*params_sent {
            let payload = build_tdlib_parameters(cfg);
            if let Some(c) = client.as_ref() {
              let _ = c.send(&payload);
            }
            *params_sent = true;
            *waiting_for_params = false;
          }
        }
      }

      if client.is_none() && (config.is_none() || lib_path.is_none()) {
        set_auth_state(app, AuthState::WaitConfig, last_state);
      }
    }
    TdlibCommand::Request { payload, respond_to } => {
      if client.is_none() {
        let _ = respond_to.send(Err(anyhow::anyhow!("TDLib еще не инициализирован")));
        return;
      }

      let mut request = payload;
      let Some(obj) = request.as_object_mut() else {
        let _ = respond_to.send(Err(anyhow::anyhow!("Запрос к TDLib должен быть объектом")));
        return;
      };

      let request_id = *next_request_id;
      *next_request_id = next_request_id.wrapping_add(1).max(1);
      obj.insert("@extra".to_string(), json!(request_id));

      pending_requests.insert(request_id, respond_to);
      if let Some(c) = client.as_ref() {
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
      for line in reader.lines().flatten() {
        let _ = tx.send(("stdout".to_string(), line));
      }
    });
  }

  if let Some(err) = stderr {
    let tx = tx.clone();
    std::thread::spawn(move || {
      let reader = BufReader::new(err);
      for line in reader.lines().flatten() {
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

fn handle_tdlib_response(
  v: &Value,
  client: &TdlibClient,
  config: &mut Option<TdlibConfig>,
  waiting_for_params: &mut bool,
  params_sent: &mut bool,
  app: &tauri::AppHandle,
  last_state: &mut Option<AuthState>
) -> anyhow::Result<()> {
  let t = v.get("@type").and_then(|v| v.as_str()).unwrap_or("");

  if t == "updateAuthorizationState" {
    if let Some(state) = v.get("authorization_state") {
      handle_auth_state(state, client, config, waiting_for_params, params_sent, app, last_state)?;
    }
    return Ok(());
  }

  if t.starts_with("authorizationState") {
    handle_auth_state(&v, client, config, waiting_for_params, params_sent, app, last_state)?;
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
