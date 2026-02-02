use chrono::Utc;
use sqlx::{SqlitePool, Row};
use ulid::Ulid;
use tokio::time::{sleep, Duration};

use crate::fsmeta::{FileMeta, parse_dir_message, parse_file_caption, make_file_caption};
use crate::telegram::{TelegramService, ChatId, HistoryMessage};

use super::dirs;

pub const UNASSIGNED_DIR_NAME: &str = "Неразобранное";

#[derive(Default, Debug, Clone)]
pub struct IndexOutcome {
  pub dir: bool,
  pub file: bool,
  pub imported: bool,
  pub skipped: bool,
  pub failed: bool
}

pub async fn index_storage_message(
  pool: &SqlitePool,
  tg: &dyn TelegramService,
  storage_chat_id: ChatId,
  msg: &HistoryMessage,
  unassigned_cache: &mut Option<(String, String)>
) -> anyhow::Result<IndexOutcome> {
  let mut out = IndexOutcome::default();

  if let Some(text) = msg.text.as_deref() {
    if let Ok(meta) = parse_dir_message(text) {
      upsert_dir(pool, &meta, msg.id, msg.date).await?;
      out.dir = true;
      return Ok(out);
    }
  }

  if let Some(caption) = msg.caption.as_deref() {
    if let Ok(meta) = parse_file_caption(caption) {
      upsert_file(pool, &meta, storage_chat_id, msg.id, msg.date, msg.file_size.unwrap_or(0)).await?;
      out.file = true;
      return Ok(out);
    }
  }

  let has_file = msg.file_size.is_some()
    || msg.file_name.as_ref().map(|v| !v.trim().is_empty()).unwrap_or(false);
  if !has_file {
    out.skipped = true;
    return Ok(out);
  }

  match import_untagged_file(pool, tg, storage_chat_id, msg, unassigned_cache).await? {
    ImportAction::Imported => {
      out.imported = true;
      out.file = true;
    }
    ImportAction::Skipped => {
      out.skipped = true;
    }
  }

  Ok(out)
}

enum ImportAction {
  Imported,
  Skipped
}

async fn import_untagged_file(
  pool: &SqlitePool,
  tg: &dyn TelegramService,
  storage_chat_id: ChatId,
  msg: &HistoryMessage,
  unassigned_cache: &mut Option<(String, String)>
) -> anyhow::Result<ImportAction> {
  if let Some(row) = sqlx::query("SELECT id FROM files WHERE tg_chat_id = ? AND tg_msg_id = ?")
    .bind(storage_chat_id)
    .bind(msg.id)
    .fetch_optional(pool)
    .await? {
    let _existing: String = row.get("id");
    return Ok(ImportAction::Skipped);
  }

  let caption_text = msg.caption.clone().unwrap_or_default();
  let mut preferred: Option<String> = None;
  let mut target: Option<(String, String)> = None;
  for tag in extract_folder_tags(&caption_text) {
    let Some(name) = normalize_tag_name(&tag) else { continue; };
    if preferred.is_none() {
      preferred = Some(name.clone());
    }
    if let Some(found) = find_dir_by_name(pool, &name).await? {
      target = Some(found);
      break;
    }
  }

  let target = if let Some(found) = target {
    found
  } else if let Some(name) = preferred {
    ensure_dir_by_name(pool, tg, storage_chat_id, &name).await?
  } else {
    if unassigned_cache.is_none() {
      *unassigned_cache = Some(ensure_dir_by_name(pool, tg, storage_chat_id, UNASSIGNED_DIR_NAME).await?);
    }
    unassigned_cache.clone().unwrap()
  };

  let file_id = Ulid::new().to_string();
  let file_name = msg.file_name.clone().filter(|v| !v.trim().is_empty())
    .unwrap_or_else(|| format!("файл_{}", msg.id));
  let size = msg.file_size.unwrap_or(0);
  let hash_short = hash_short_from_seed(&format!("{storage_chat_id}:{msg_id}:{file_name}:{size}", msg_id = msg.id));
  let caption = make_file_caption_with_tag(
    &FileMeta {
      dir_id: target.0.clone(),
      file_id: file_id.clone(),
      name: file_name.clone(),
      hash_short: hash_short.clone()
    },
    Some(target.1.as_str())
  );

  if let Err(e) = edit_caption_with_retry(tg, storage_chat_id, msg.id, &caption).await {
    tracing::warn!(
      event = "storage_import_edit_failed",
      message_id = msg.id,
      error = %e,
      "Не удалось обновить подпись сообщения, оставляю оригинал"
    );
  }

  if !message_exists_with_retry(tg, storage_chat_id, msg.id).await {
    tracing::warn!(
      event = "storage_import_message_missing",
      message_id = msg.id,
      "Сообщение не найдено, импорт пропущен"
    );
    return Ok(ImportAction::Skipped);
  }

  let created_at = if msg.date > 0 { msg.date } else { Utc::now().timestamp() };
  let inserted = sqlx::query(
    "INSERT INTO files(id, dir_id, name, size, hash, tg_chat_id, tg_msg_id, created_at)
     VALUES(?, ?, ?, ?, ?, ?, ?, ?)"
  )
    .bind(&file_id)
    .bind(&target.0)
    .bind(&file_name)
    .bind(size)
    .bind(&hash_short)
    .bind(storage_chat_id)
    .bind(msg.id)
    .bind(created_at)
    .execute(pool)
    .await;

  match inserted {
    Ok(_) => Ok(ImportAction::Imported),
    Err(e) => {
      tracing::warn!(
        event = "storage_import_db_failed",
        file_id = file_id.as_str(),
        error = %e,
        "Не удалось сохранить импортированный файл"
      );
      Ok(ImportAction::Skipped)
    }
  }
}

async fn edit_caption_with_retry(
  tg: &dyn TelegramService,
  chat_id: ChatId,
  message_id: i64,
  caption: &str
) -> Result<(), String> {
  match tg.edit_message_caption(chat_id, message_id, caption.to_string()).await {
    Ok(()) => Ok(()),
    Err(first) => {
      sleep(Duration::from_millis(600)).await;
      match tg.edit_message_caption(chat_id, message_id, caption.to_string()).await {
        Ok(()) => Ok(()),
        Err(second) => Err(format!("{first}; {second}"))
      }
    }
  }
}

async fn message_exists_with_retry(
  tg: &dyn TelegramService,
  chat_id: ChatId,
  message_id: i64
) -> bool {
  let delays = [150u64, 500, 1000, 1500];
  for (idx, delay) in delays.iter().enumerate() {
    match tg.message_exists(chat_id, message_id).await {
      Ok(true) => return true,
      Ok(false) => {}
      Err(e) => {
        if idx == delays.len() - 1 {
          tracing::debug!(
            event = "storage_import_message_check_failed",
            message_id = message_id,
            error = %e,
            "Не удалось проверить сообщение"
          );
        }
      }
    }
    sleep(Duration::from_millis(*delay)).await;
  }
  false
}

pub async fn upsert_dir(pool: &SqlitePool, meta: &crate::fsmeta::DirMeta, msg_id: i64, date: i64) -> anyhow::Result<()> {
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

pub async fn upsert_file(
  pool: &SqlitePool,
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
     ON CONFLICT(id) DO UPDATE SET dir_id=excluded.dir_id, name=excluded.name, size=excluded.size, hash=excluded.hash, tg_chat_id=excluded.tg_chat_id, tg_msg_id=excluded.tg_msg_id"
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

async fn ensure_dir_placeholder(pool: &SqlitePool, dir_id: &str, date: i64) -> anyhow::Result<()> {
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

async fn find_dir_by_name(pool: &SqlitePool, name: &str) -> anyhow::Result<Option<(String, String)>> {
  let row = sqlx::query(
    "SELECT id, name FROM directories
     WHERE lower(name) = lower(?)
     ORDER BY (parent_id IS NULL) DESC, updated_at DESC
     LIMIT 1"
  )
    .bind(name)
    .fetch_optional(pool)
    .await?;
  Ok(row.map(|r| (r.get::<String,_>("id"), r.get::<String,_>("name"))))
}

async fn ensure_dir_by_name(
  pool: &SqlitePool,
  tg: &dyn TelegramService,
  storage_chat_id: ChatId,
  name: &str
) -> anyhow::Result<(String, String)> {
  if let Some(found) = find_dir_by_name(pool, name).await? {
    return Ok(found);
  }
  let id = dirs::create_dir(pool, tg, storage_chat_id, None, name.to_string()).await?;
  Ok((id, name.to_string()))
}

fn hash_short_from_seed(seed: &str) -> String {
  use sha2::{Digest, Sha256};
  let mut hasher = Sha256::new();
  hasher.update(seed.as_bytes());
  let digest = hex::encode(hasher.finalize());
  digest.chars().take(8).collect()
}

fn make_file_caption_with_tag(meta: &FileMeta, dir_name: Option<&str>) -> String {
  let base = make_file_caption(meta);
  if let Some(tag) = dir_name.and_then(folder_hashtag) {
    format!("{base} {tag}")
  } else {
    base
  }
}

fn folder_hashtag(name: &str) -> Option<String> {
  let trimmed = name.trim();
  if trimmed.is_empty() {
    return None;
  }
  let mut out = String::new();
  let mut last_underscore = false;
  for ch in trimmed.chars() {
    if ch.is_alphanumeric() {
      out.push(ch);
      last_underscore = false;
    } else if ch == '_' || ch.is_whitespace() || ch == '-' || ch == '.' {
      if !last_underscore {
        out.push('_');
        last_underscore = true;
      }
    }
  }
  let cleaned = out.trim_matches('_').to_string();
  if cleaned.is_empty() {
    None
  } else {
    Some(format!("#{cleaned}"))
  }
}

fn is_reserved_tag(tag: &str) -> bool {
  matches!(tag, "ocltg" | "v1" | "file" | "dir")
}

fn extract_folder_tags(caption: &str) -> Vec<String> {
  let mut tags = Vec::new();
  let mut chars = caption.chars().peekable();
  while let Some(ch) = chars.next() {
    if ch != '#' {
      continue;
    }
    let mut tag = String::new();
    while let Some(&c) = chars.peek() {
      if c.is_alphanumeric() || c == '_' || c == '-' {
        tag.push(c);
        chars.next();
      } else {
        break;
      }
    }
    if tag.is_empty() {
      continue;
    }
    let lowered = tag.to_lowercase();
    if is_reserved_tag(lowered.as_str()) {
      continue;
    }
    tags.push(tag);
  }
  tags
}

fn normalize_tag_name(tag: &str) -> Option<String> {
  let mut out = String::new();
  let mut last_space = false;
  for ch in tag.chars() {
    let mapped = if ch == '_' || ch == '-' { ' ' } else { ch };
    if mapped.is_whitespace() {
      if !last_space {
        out.push(' ');
        last_space = true;
      }
    } else {
      out.push(mapped);
      last_space = false;
    }
  }
  let cleaned = out.trim().to_string();
  if cleaned.is_empty() { None } else { Some(cleaned) }
}
