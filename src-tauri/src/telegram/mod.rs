use std::sync::Arc;

use crate::paths::Paths;

pub type ChatId = i64;
pub type MessageId = i64;

#[derive(Debug, Clone)]
pub struct UploadedMessage {
  pub chat_id: ChatId,
  pub message_id: MessageId,
  pub caption_or_text: String
}

#[derive(Debug, Clone)]
pub struct HistoryMessage {
  pub id: MessageId,
  pub date: i64,
  pub text: Option<String>,
  pub caption: Option<String>,
  pub file_size: Option<i64>
}

#[derive(Debug, Clone)]
pub struct SearchMessagesResult {
  pub total_count: Option<i64>,
  pub next_from_message_id: MessageId,
  pub messages: Vec<HistoryMessage>
}

#[derive(thiserror::Error, Debug)]
pub enum TgError {
  #[error("не реализовано")]
  NotImplemented,
  #[error("требуется авторизация")]
  AuthRequired,
  #[error("ошибка ввода-вывода: {0}")]
  Io(#[from] std::io::Error),
  #[error("{0}")]
  Other(String)
}

#[async_trait::async_trait]
pub trait TelegramService: Send + Sync {
  async fn auth_start(&self, phone: String) -> Result<(), TgError>;
  async fn auth_submit_code(&self, code: String) -> Result<(), TgError>;
  async fn auth_submit_password(&self, password: String) -> Result<(), TgError>;
  async fn configure(&self, api_id: i32, api_hash: String, tdlib_path: Option<String>) -> Result<(), TgError>;

  async fn storage_check_channel(&self, chat_id: ChatId) -> Result<bool, TgError>;
  async fn storage_get_or_create_channel(&self) -> Result<ChatId, TgError>;
  async fn storage_create_channel(&self) -> Result<ChatId, TgError>;
  async fn storage_delete_channel(&self, chat_id: ChatId) -> Result<(), TgError>;
  async fn search_storage_messages(&self, chat_id: ChatId, from_message_id: MessageId, limit: i32)
    -> Result<SearchMessagesResult, TgError>;

  async fn send_text_message(&self, chat_id: ChatId, text: String) -> Result<UploadedMessage, TgError>;
  async fn send_dir_message(&self, chat_id: ChatId, text: String) -> Result<UploadedMessage, TgError>;
  async fn send_file(&self, chat_id: ChatId, path: std::path::PathBuf, caption: String) -> Result<UploadedMessage, TgError>;
  async fn copy_messages(&self, from_chat_id: ChatId, to_chat_id: ChatId, message_ids: Vec<MessageId>)
    -> Result<Vec<Option<MessageId>>, TgError>;

  async fn download_message_file(&self, chat_id: ChatId, message_id: MessageId, target: std::path::PathBuf) -> Result<std::path::PathBuf, TgError>;
}

#[cfg(feature = "mock_telegram")]
mod mock;
#[cfg(feature = "mock_telegram")]
pub use mock::MockTelegram;

#[cfg(feature = "tdlib")]
mod tdlib;

pub fn make_telegram_service(
  paths: Paths,
  app: tauri::AppHandle,
  tg_settings: Option<crate::settings::TgSettings>,
  tdlib_path: Option<String>
) -> anyhow::Result<Arc<dyn TelegramService>> {
  #[cfg(feature = "mock_telegram")]
  {
    return Ok(Arc::new(MockTelegram::new(paths, app)));
  }

  #[cfg(all(not(feature = "mock_telegram"), feature = "tdlib"))]
  {
    return Ok(Arc::new(tdlib::TdlibTelegram::new(paths, app, tg_settings, tdlib_path)?));
  }

  #[cfg(all(not(feature = "mock_telegram"), not(feature = "tdlib")))]
  {
    Err(anyhow::anyhow!(
      "Не выбран Telegram backend. Включи фичу 'mock_telegram' или 'tdlib'."
    ))
  }
}
