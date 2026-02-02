use chrono::Utc;
use sqlx::{SqlitePool, Row};
use ulid::Ulid;
use std::path::Path;

use crate::fsmeta::{FileMeta, make_file_caption, parse_file_caption};
use crate::telegram::{TelegramService, ChatId};
use crate::app::dirs::dir_exists;

fn hash_short(path: &Path) -> anyhow::Result<String> {
  use sha2::{Digest, Sha256};
  use std::io::Read;

  let mut file = std::fs::File::open(path)?;
  let mut hasher = Sha256::new();
  let mut buf = [0u8; 8192];
  loop {
    let n = file.read(&mut buf)?;
    if n == 0 {
      break;
    }
    hasher.update(&buf[..n]);
  }
  let digest = hex::encode(hasher.finalize());
  Ok(digest.chars().take(8).collect())
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FileItem {
  pub id: String,
  pub dir_id: String,
  pub name: String,
  pub size: i64,
  pub hash: String,
  pub tg_chat_id: i64,
  pub tg_msg_id: i64,
  pub created_at: i64
}

pub async fn list_files(pool: &SqlitePool, dir_id: &str) -> anyhow::Result<Vec<FileItem>> {
  let rows = sqlx::query(
    "SELECT id, dir_id, name, size, hash, tg_chat_id, tg_msg_id, created_at FROM files WHERE dir_id = ? ORDER BY name"
  )
    .bind(dir_id)
    .fetch_all(pool)
    .await?;

  let mut out = Vec::with_capacity(rows.len());
  for row in rows {
    out.push(FileItem {
      id: row.get::<String,_>("id"),
      dir_id: row.get::<String,_>("dir_id"),
      name: row.get::<String,_>("name"),
      size: row.get::<i64,_>("size"),
      hash: row.get::<String,_>("hash"),
      tg_chat_id: row.get::<i64,_>("tg_chat_id"),
      tg_msg_id: row.get::<i64,_>("tg_msg_id"),
      created_at: row.get::<i64,_>("created_at")
    });
  }
  Ok(out)
}

pub async fn upload_file(
  pool: &SqlitePool,
  tg: &dyn TelegramService,
  chat_id: ChatId,
  dir_id: &str,
  path: &Path
) -> anyhow::Result<String> {
  if !dir_exists(pool, dir_id).await? {
    return Err(anyhow::anyhow!("Папка не найдена"));
  }
  if !path.is_file() {
    return Err(anyhow::anyhow!("Файл не найден"));
  }

  let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
  let size = path.metadata().map(|m| m.len() as i64).unwrap_or(0);
  let hash_short = hash_short(path)?;
  let id = Ulid::new().to_string();

  let dir_name = fetch_dir_name(pool, dir_id).await?;
  let caption = make_file_caption_with_tag(
    &FileMeta {
      dir_id: dir_id.to_string(),
      file_id: id.clone(),
      name: file_name.clone(),
      hash_short: hash_short.clone()
    },
    dir_name.as_deref()
  );

  let uploaded = tg.send_file(chat_id, path.to_path_buf(), caption).await?;
  let created_at = Utc::now().timestamp();

  sqlx::query(
    "INSERT INTO files(id, dir_id, name, size, hash, tg_chat_id, tg_msg_id, created_at)\n     VALUES(?, ?, ?, ?, ?, ?, ?, ?)"
  )
    .bind(&id)
    .bind(dir_id)
    .bind(&file_name)
    .bind(size)
    .bind(hash_short)
    .bind(uploaded.chat_id)
    .bind(uploaded.message_id)
    .bind(created_at)
    .execute(pool)
    .await?;

  Ok(id)
}

pub async fn move_file(
  pool: &SqlitePool,
  tg: &dyn TelegramService,
  storage_chat_id: ChatId,
  file_id: &str,
  new_dir_id: &str
) -> anyhow::Result<()> {
  if !dir_exists(pool, new_dir_id).await? {
    return Err(anyhow::anyhow!("Папка не найдена"));
  }
  let row = sqlx::query("SELECT id, dir_id, name, hash, tg_chat_id, tg_msg_id FROM files WHERE id = ?")
    .bind(file_id)
    .fetch_optional(pool)
    .await?;
  let Some(row) = row else {
    return Err(anyhow::anyhow!("Файл не найден"));
  };
  let current_dir: String = row.get("dir_id");
  if current_dir == new_dir_id {
    return Ok(());
  }
  let name: String = row.get("name");
  let hash: String = row.get("hash");
  let mut msg_id: i64 = row.get("tg_msg_id");
  let mut msg_chat_id: i64 = row.get("tg_chat_id");
  let dir_name = fetch_dir_name(pool, new_dir_id).await?;

  let caption = make_file_caption_with_tag(
    &FileMeta {
      dir_id: new_dir_id.to_string(),
      file_id: file_id.to_string(),
      name: name.clone(),
      hash_short: hash.clone()
    },
    dir_name.as_deref()
  );

  let mut edit_error = match tg.edit_message_caption(msg_chat_id, msg_id, caption.clone()).await {
    Ok(()) => {
      sqlx::query("UPDATE files SET dir_id = ?, tg_chat_id = ?, tg_msg_id = ? WHERE id = ?")
        .bind(new_dir_id)
        .bind(msg_chat_id)
        .bind(msg_id)
        .bind(file_id)
        .execute(pool)
        .await?;
      return Ok(());
    }
    Err(e) => {
      tracing::warn!(
        event = "file_caption_update_failed",
        file_id = file_id,
        error = %e,
        "Не удалось обновить подпись файла в TG, пробую переотправку"
      );
      Some(e.to_string())
    }
  };

  if let Some((found_chat_id, found_msg_id)) = find_file_message(tg, msg_chat_id, storage_chat_id, file_id).await? {
    if found_chat_id != msg_chat_id || found_msg_id != msg_id {
      msg_chat_id = found_chat_id;
      msg_id = found_msg_id;
      sqlx::query("UPDATE files SET tg_chat_id = ?, tg_msg_id = ? WHERE id = ?")
        .bind(msg_chat_id)
        .bind(msg_id)
        .bind(file_id)
        .execute(pool)
        .await?;
    }
    match tg.edit_message_caption(msg_chat_id, msg_id, caption.clone()).await {
      Ok(()) => {
        sqlx::query("UPDATE files SET dir_id = ?, tg_chat_id = ?, tg_msg_id = ? WHERE id = ?")
          .bind(new_dir_id)
          .bind(msg_chat_id)
          .bind(msg_id)
          .bind(file_id)
          .execute(pool)
          .await?;
        return Ok(());
      }
      Err(e) => {
        edit_error = Some(e.to_string());
      }
    }
  }

  let resend_error = match tg.send_file_from_message(msg_chat_id, msg_id, caption.clone()).await {
    Ok(uploaded) => {
      let _ = tg.delete_messages(msg_chat_id, vec![msg_id], true).await;
      sqlx::query("UPDATE files SET dir_id = ?, tg_chat_id = ?, tg_msg_id = ? WHERE id = ?")
        .bind(new_dir_id)
        .bind(uploaded.chat_id)
        .bind(uploaded.message_id)
        .bind(file_id)
        .execute(pool)
        .await?;
      return Ok(());
    }
    Err(e) => {
      tracing::warn!(
        event = "file_resend_for_move_failed",
        file_id = file_id,
        error = %e,
        "Не удалось переотправить файл с новой подписью, пробую копирование"
      );
      Some(e.to_string())
    }
  };

  let ids = tg
    .copy_messages(msg_chat_id, msg_chat_id, vec![msg_id])
    .await
    .map_err(|e| anyhow::anyhow!("Не удалось скопировать файл для обновления подписи: {e}"))?;
  let new_msg_id = ids
    .into_iter()
    .next()
    .flatten()
    .ok_or_else(|| {
      let mut detail = String::new();
      if let Some(err) = edit_error.as_deref() {
        detail.push_str(&format!(" Ошибка редактирования подписи: {err}."));
      }
      if let Some(err) = resend_error.as_deref() {
        detail.push_str(&format!(" Ошибка переотправки: {err}."));
      }
      anyhow::anyhow!(
        "TDLib не вернул id скопированного сообщения.{detail} Возможно, в канале включена защита контента."
      )
    })?;

  if let Err(e) = tg.edit_message_caption(msg_chat_id, new_msg_id, caption).await {
    tracing::warn!(
      event = "file_caption_update_failed",
      file_id = file_id,
      error = %e,
      "Не удалось обновить подпись файла после копирования"
    );
    let _ = tg.delete_messages(msg_chat_id, vec![new_msg_id], true).await;
    return Err(anyhow::anyhow!("Не удалось обновить подпись файла после копирования"));
  }

  let _ = tg.delete_messages(msg_chat_id, vec![msg_id], true).await;

  sqlx::query("UPDATE files SET dir_id = ?, tg_chat_id = ?, tg_msg_id = ? WHERE id = ?")
    .bind(new_dir_id)
    .bind(msg_chat_id)
    .bind(new_msg_id)
    .bind(file_id)
    .execute(pool)
    .await?;
  Ok(())
}

pub async fn delete_file(
  pool: &SqlitePool,
  tg: &dyn TelegramService,
  file_id: &str
) -> anyhow::Result<()> {
  let row = sqlx::query("SELECT tg_msg_id, tg_chat_id FROM files WHERE id = ?")
    .bind(file_id)
    .fetch_optional(pool)
    .await?;
  let Some(row) = row else {
    return Err(anyhow::anyhow!("Файл не найден"));
  };
  let msg_id: i64 = row.get("tg_msg_id");
  let msg_chat_id: i64 = row.get("tg_chat_id");
  if let Err(e) = tg.delete_messages(msg_chat_id, vec![msg_id], true).await {
    tracing::warn!(event = "file_delete_message_failed", file_id = file_id, error = %e, "Не удалось удалить сообщение файла в TG");
  }
  sqlx::query("DELETE FROM files WHERE id = ?")
    .bind(file_id)
    .execute(pool)
    .await?;
  Ok(())
}

pub async fn delete_files(
  pool: &SqlitePool,
  tg: &dyn TelegramService,
  file_ids: &[String]
) -> anyhow::Result<()> {
  if file_ids.is_empty() {
    return Ok(());
  }
  let mut grouped: std::collections::HashMap<i64, Vec<i64>> = std::collections::HashMap::new();
  for id in file_ids {
    if let Some(row) = sqlx::query("SELECT tg_msg_id, tg_chat_id FROM files WHERE id = ?")
      .bind(id)
      .fetch_optional(pool)
      .await? {
      let msg_id = row.get::<i64,_>("tg_msg_id");
      let msg_chat_id = row.get::<i64,_>("tg_chat_id");
      grouped.entry(msg_chat_id).or_default().push(msg_id);
    }
  }
  if !grouped.is_empty() {
    for (msg_chat_id, msg_ids) in grouped {
      if let Err(e) = tg.delete_messages(msg_chat_id, msg_ids, true).await {
        tracing::warn!(event = "file_delete_many_message_failed", count = file_ids.len(), error = %e, "Не удалось удалить сообщения файлов в TG");
      }
    }
  }
  for id in file_ids {
    sqlx::query("DELETE FROM files WHERE id = ?")
      .bind(id)
      .execute(pool)
      .await?;
  }
  Ok(())
}

pub fn build_message_link(chat_id: i64, message_id: i64) -> anyhow::Result<String> {
  if chat_id >= 0 {
    return Err(anyhow::anyhow!("Ссылка доступна только для сообщений каналов"));
  }
  let abs = chat_id.abs();
  let internal = abs
    .checked_sub(1_000_000_000_000)
    .filter(|v| *v > 0)
    .ok_or_else(|| anyhow::anyhow!("Не удалось определить ссылку для этого чата"))?;
  Ok(format!("https://t.me/c/{internal}/{message_id}"))
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

async fn fetch_dir_name(pool: &SqlitePool, dir_id: &str) -> anyhow::Result<Option<String>> {
  let row = sqlx::query("SELECT name FROM directories WHERE id = ?")
    .bind(dir_id)
    .fetch_optional(pool)
    .await?;
  Ok(row.map(|r| r.get::<String,_>("name")))
}

pub async fn find_file_message(
  tg: &dyn TelegramService,
  msg_chat_id: ChatId,
  storage_chat_id: ChatId,
  file_id: &str
) -> anyhow::Result<Option<(ChatId, i64)>> {
  let query = format!("f={file_id}");
  let mut from_message_id: i64 = 0;

  for _ in 0..8 {
    let batch = match tg
      .search_chat_messages(msg_chat_id, query.clone(), from_message_id, 100)
      .await {
      Ok(v) => v,
      Err(_) => break
    };
    for msg in batch.messages {
      if let Some(caption) = msg.caption.as_deref() {
        if let Ok(meta) = parse_file_caption(caption) {
          if meta.file_id == file_id {
            return Ok(Some((msg_chat_id, msg.id)));
          }
        }
      }
    }
    if batch.next_from_message_id == 0 {
      break;
    }
    from_message_id = batch.next_from_message_id;
  }

  if storage_chat_id != msg_chat_id {
    let mut from_message_id: i64 = 0;
    for _ in 0..8 {
      let batch = match tg
        .search_chat_messages(storage_chat_id, query.clone(), from_message_id, 100)
        .await {
        Ok(v) => v,
        Err(_) => break
      };
      for msg in batch.messages {
        if let Some(caption) = msg.caption.as_deref() {
          if let Ok(meta) = parse_file_caption(caption) {
            if meta.file_id == file_id {
              return Ok(Some((storage_chat_id, msg.id)));
            }
          }
        }
      }
      if batch.next_from_message_id == 0 {
        break;
      }
      from_message_id = batch.next_from_message_id;
    }
  }

  Ok(None)
}
