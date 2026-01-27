use crate::paths::Paths;
use super::{ChatId, MessageId, TelegramService, TgError, UploadedMessage};

pub struct TdlibTelegram {
  _paths: Paths,
  _app: tauri::AppHandle
}

impl TdlibTelegram {
  pub fn new(paths: Paths, app: tauri::AppHandle) -> anyhow::Result<Self> {
    // TODO: init TDLib (libtdjson) and spawn receive-loop.
    Ok(Self { _paths: paths, _app: app })
  }
}

#[async_trait::async_trait]
impl TelegramService for TdlibTelegram {
  async fn auth_start(&self, _phone: String) -> Result<(), TgError> { Err(TgError::NotImplemented) }
  async fn auth_submit_code(&self, _code: String) -> Result<(), TgError> { Err(TgError::NotImplemented) }
  async fn auth_submit_password(&self, _password: String) -> Result<(), TgError> { Err(TgError::NotImplemented) }

  async fn storage_get_or_create_channel(&self) -> Result<ChatId, TgError> { Err(TgError::NotImplemented) }

  async fn send_dir_message(&self, _chat_id: ChatId, _text: String) -> Result<UploadedMessage, TgError> { Err(TgError::NotImplemented) }
  async fn send_file(&self, _chat_id: ChatId, _path: std::path::PathBuf, _caption: String) -> Result<UploadedMessage, TgError> { Err(TgError::NotImplemented) }

  async fn download_message_file(&self, _chat_id: ChatId, _message_id: MessageId, _target: std::path::PathBuf)
    -> Result<std::path::PathBuf, TgError> { Err(TgError::NotImplemented) }
}
