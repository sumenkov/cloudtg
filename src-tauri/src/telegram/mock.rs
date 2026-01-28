use std::{collections::VecDeque, path::PathBuf};

use parking_lot::Mutex;

use crate::paths::Paths;
use super::{ChatId, MessageId, TelegramService, TgError, UploadedMessage};

pub struct MockTelegram {
  paths: Paths,
  _app: tauri::AppHandle,
  chat_id: Mutex<ChatId>,
  next_msg_id: Mutex<MessageId>,
  messages: Mutex<VecDeque<UploadedMessage>>,
  authed: Mutex<bool>
}

impl MockTelegram {
  pub fn new(paths: Paths, app: tauri::AppHandle) -> Self {
    Self {
      paths,
      _app: app,
      chat_id: Mutex::new(777),
      next_msg_id: Mutex::new(1),
      messages: Mutex::new(VecDeque::new()),
      authed: Mutex::new(true)
    }
  }

  fn alloc_msg_id(&self) -> MessageId {
    let mut g = self.next_msg_id.lock();
    let id = *g;
    *g += 1;
    id
  }
}

#[async_trait::async_trait]
impl TelegramService for MockTelegram {
  async fn auth_start(&self, _phone: String) -> Result<(), TgError> { *self.authed.lock() = true; Ok(()) }
  async fn auth_submit_code(&self, _code: String) -> Result<(), TgError> { *self.authed.lock() = true; Ok(()) }
  async fn auth_submit_password(&self, _password: String) -> Result<(), TgError> { *self.authed.lock() = true; Ok(()) }
  async fn configure(&self, _api_id: i32, _api_hash: String, _tdlib_path: Option<String>) -> Result<(), TgError> { Ok(()) }

  async fn storage_check_channel(&self, _chat_id: ChatId) -> Result<bool, TgError> {
    Ok(*self.authed.lock())
  }

  async fn storage_get_or_create_channel(&self) -> Result<ChatId, TgError> {
    if !*self.authed.lock() { return Err(TgError::AuthRequired); }
    Ok(*self.chat_id.lock())
  }

  async fn storage_create_channel(&self) -> Result<ChatId, TgError> {
    if !*self.authed.lock() { return Err(TgError::AuthRequired); }
    let mut guard = self.chat_id.lock();
    *guard += 1;
    Ok(*guard)
  }

  async fn send_text_message(&self, chat_id: ChatId, text: String) -> Result<UploadedMessage, TgError> {
    let msg = UploadedMessage { chat_id, message_id: self.alloc_msg_id(), caption_or_text: text };
    self.messages.lock().push_back(msg.clone());
    Ok(msg)
  }

  async fn send_dir_message(&self, chat_id: ChatId, text: String) -> Result<UploadedMessage, TgError> {
    let msg = UploadedMessage { chat_id, message_id: self.alloc_msg_id(), caption_or_text: text };
    self.messages.lock().push_back(msg.clone());
    Ok(msg)
  }

  async fn send_file(&self, chat_id: ChatId, path: PathBuf, caption: String) -> Result<UploadedMessage, TgError> {
    let uploads_dir = self.paths.cache_dir.join("mock_uploads");
    std::fs::create_dir_all(&uploads_dir).map_err(TgError::Io)?;
    let filename = path.file_name().unwrap_or_default().to_string_lossy().to_string();
    let dest = uploads_dir.join(format!("{}-{}", self.alloc_msg_id(), filename));
    std::fs::copy(&path, &dest).map_err(TgError::Io)?;

    let msg = UploadedMessage { chat_id, message_id: self.alloc_msg_id(), caption_or_text: caption };
    self.messages.lock().push_back(msg.clone());
    Ok(msg)
  }

  async fn copy_messages(
    &self,
    _from_chat_id: ChatId,
    to_chat_id: ChatId,
    message_ids: Vec<MessageId>
  ) -> Result<Vec<Option<MessageId>>, TgError> {
    let mut out = Vec::with_capacity(message_ids.len());
    for _ in message_ids {
      let msg = UploadedMessage { chat_id: to_chat_id, message_id: self.alloc_msg_id(), caption_or_text: "mock copy".into() };
      self.messages.lock().push_back(msg.clone());
      out.push(Some(msg.message_id));
    }
    Ok(out)
  }

  async fn download_message_file(&self, _chat_id: ChatId, _message_id: MessageId, target: PathBuf) -> Result<PathBuf, TgError> {
    if let Some(parent) = target.parent() { std::fs::create_dir_all(parent).map_err(TgError::Io)?; }
    if !target.exists() {
      std::fs::write(&target, b"mock download: tdlib not enabled\n").map_err(TgError::Io)?;
    }
    Ok(target)
  }
}
