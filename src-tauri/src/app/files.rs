use chrono::Utc;
use crate::sqlx::{self, QueryBuilder, Row};
use sqlx_sqlite::SqlitePool;
use ulid::Ulid;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::fsmeta::{FileMeta, make_file_caption, parse_file_caption};
use crate::telegram::{TelegramService, ChatId};
use crate::app::dirs::dir_exists;
use crate::paths::Paths;

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
  pub local_size: Option<i64>,
  pub is_downloaded: bool,
  pub hash: String,
  pub tg_chat_id: i64,
  pub tg_msg_id: i64,
  pub created_at: i64,
  pub is_broken: bool
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepairFileResult {
  Repaired,
  NeedFile
}

pub async fn list_files(pool: &SqlitePool, paths: &Paths, dir_id: &str) -> anyhow::Result<Vec<FileItem>> {
  let rows = sqlx::query(
    "SELECT id, dir_id, name, size, hash, tg_chat_id, tg_msg_id, created_at, is_broken FROM files WHERE dir_id = ? ORDER BY name"
  )
    .bind(dir_id)
    .fetch_all(pool)
    .await?;

  let dir_path = build_dir_path(pool, dir_id).await?;
  let mut out = Vec::with_capacity(rows.len());
  for row in rows {
    let name: String = row.get("name");
    let size: i64 = row.get("size");
    let (is_downloaded, local_size) = local_download_info(paths, &dir_path, &name, size);
    out.push(FileItem {
      id: row.get::<String,_>("id"),
      dir_id: row.get::<String,_>("dir_id"),
      name,
      size,
      local_size,
      is_downloaded,
      hash: row.get::<String,_>("hash"),
      tg_chat_id: row.get::<i64,_>("tg_chat_id"),
      tg_msg_id: row.get::<i64,_>("tg_msg_id"),
      created_at: row.get::<i64,_>("created_at"),
      is_broken: row.get::<i64,_>("is_broken") != 0
    });
  }
  Ok(out)
}

pub async fn search_files(
  pool: &SqlitePool,
  paths: &Paths,
  dir_id: Option<&str>,
  name: Option<&str>,
  file_type: Option<&str>,
  limit: Option<i64>
) -> anyhow::Result<Vec<FileItem>> {
  let mut builder = QueryBuilder::new(
    "SELECT id, dir_id, name, size, hash, tg_chat_id, tg_msg_id, created_at, is_broken FROM files"
  );
  let dir_id = dir_id.filter(|v| !v.trim().is_empty() && *v != "ROOT");
  let name = name.map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
  let file_type = file_type
    .map(|v| v.trim().to_string())
    .filter(|v| !v.is_empty())
    .and_then(|v| {
      let trimmed = v.trim_start_matches('.').trim().to_string();
      if trimmed.is_empty() { None } else { Some(trimmed) }
    });

  builder.push(" WHERE 1=1");

  if let Some(dir_id) = dir_id {
    builder.push(" AND dir_id = ").push_bind(dir_id);
  }

  if let Some(name) = name {
    builder
      .push(" AND lower(name) LIKE ")
      .push_bind(format!("%{}%", name.to_lowercase()));
  }

  if let Some(file_type) = file_type {
    builder
      .push(" AND lower(name) LIKE ")
      .push_bind(format!("%.{file_type}", file_type = file_type.to_lowercase()));
  }

  builder.push(" ORDER BY name");
  builder.push(" LIMIT ").push_bind(limit.unwrap_or(500).max(1));

  let rows = builder.build().fetch_all(pool).await?;
  let mut out = Vec::with_capacity(rows.len());
  let mut dir_paths: HashMap<String, PathBuf> = HashMap::new();
  for row in rows {
    let dir_id: String = row.get("dir_id");
    let name: String = row.get("name");
    let size: i64 = row.get("size");
    let dir_path = if let Some(cached) = dir_paths.get(&dir_id) {
      cached.clone()
    } else {
      let built = build_dir_path(pool, &dir_id).await?;
      dir_paths.insert(dir_id.clone(), built.clone());
      built
    };
    let (is_downloaded, local_size) = local_download_info(paths, &dir_path, &name, size);
    out.push(FileItem {
      id: row.get::<String,_>("id"),
      dir_id,
      name,
      size,
      local_size,
      is_downloaded,
      hash: row.get::<String,_>("hash"),
      tg_chat_id: row.get::<i64,_>("tg_chat_id"),
      tg_msg_id: row.get::<i64,_>("tg_msg_id"),
      created_at: row.get::<i64,_>("created_at"),
      is_broken: row.get::<i64,_>("is_broken") != 0
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
    "INSERT INTO files(id, dir_id, name, size, hash, tg_chat_id, tg_msg_id, created_at, is_broken)
     VALUES(?, ?, ?, ?, ?, ?, ?, ?, 0)
     ON CONFLICT(id) DO UPDATE SET dir_id=excluded.dir_id, name=excluded.name, size=excluded.size, hash=excluded.hash, tg_chat_id=excluded.tg_chat_id, tg_msg_id=excluded.tg_msg_id, is_broken=0"
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
      sqlx::query("UPDATE files SET dir_id = ?, tg_chat_id = ?, tg_msg_id = ?, is_broken = 0 WHERE id = ?")
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
      sqlx::query("UPDATE files SET tg_chat_id = ?, tg_msg_id = ?, is_broken = 0 WHERE id = ?")
        .bind(msg_chat_id)
        .bind(msg_id)
        .bind(file_id)
        .execute(pool)
        .await?;
    }
    match tg.edit_message_caption(msg_chat_id, msg_id, caption.clone()).await {
      Ok(()) => {
        sqlx::query("UPDATE files SET dir_id = ?, tg_chat_id = ?, tg_msg_id = ?, is_broken = 0 WHERE id = ?")
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
      sqlx::query("UPDATE files SET dir_id = ?, tg_chat_id = ?, tg_msg_id = ?, is_broken = 0 WHERE id = ?")
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

  sqlx::query("UPDATE files SET dir_id = ?, tg_chat_id = ?, tg_msg_id = ?, is_broken = 0 WHERE id = ?")
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
  paths: &Paths,
  file_id: &str
) -> anyhow::Result<()> {
  let row = sqlx::query("SELECT tg_msg_id, tg_chat_id, dir_id, name, size FROM files WHERE id = ?")
    .bind(file_id)
    .fetch_optional(pool)
    .await?;
  let Some(row) = row else {
    return Err(anyhow::anyhow!("Файл не найден"));
  };
  let msg_id: i64 = row.get("tg_msg_id");
  let msg_chat_id: i64 = row.get("tg_chat_id");
  let dir_id: String = row.get("dir_id");
  let name: String = row.get("name");
  let size: i64 = row.get("size");
  if let Err(e) = tg.delete_messages(msg_chat_id, vec![msg_id], true).await {
    tracing::warn!(event = "file_delete_message_failed", file_id = file_id, error = %e, "Не удалось удалить сообщение файла в TG");
  }
  if let Err(e) = remove_local_download(pool, paths, &dir_id, &name, size).await {
    tracing::warn!(event = "file_delete_local_failed", file_id = file_id, error = %e, "Не удалось удалить локальный файл");
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
  paths: &Paths,
  file_ids: &[String]
) -> anyhow::Result<()> {
  if file_ids.is_empty() {
    return Ok(());
  }
  struct Row {
    id: String,
    dir_id: String,
    name: String,
    size: i64
  }
  let mut rows: Vec<Row> = Vec::new();
  let mut grouped: std::collections::HashMap<i64, Vec<i64>> = std::collections::HashMap::new();
  for id in file_ids {
    if let Some(row) = sqlx::query("SELECT tg_msg_id, tg_chat_id, dir_id, name, size FROM files WHERE id = ?")
      .bind(id)
      .fetch_optional(pool)
      .await? {
      let msg_id = row.get::<i64,_>("tg_msg_id");
      let msg_chat_id = row.get::<i64,_>("tg_chat_id");
      let dir_id = row.get::<String,_>("dir_id");
      let name = row.get::<String,_>("name");
      let size = row.get::<i64,_>("size");
      grouped.entry(msg_chat_id).or_default().push(msg_id);
      rows.push(Row { id: id.clone(), dir_id, name, size });
    }
  }
  if !grouped.is_empty() {
    for (msg_chat_id, msg_ids) in grouped {
      if let Err(e) = tg.delete_messages(msg_chat_id, msg_ids, true).await {
        tracing::warn!(event = "file_delete_many_message_failed", count = file_ids.len(), error = %e, "Не удалось удалить сообщения файлов в TG");
      }
    }
  }
  for row in rows {
    if let Err(e) = remove_local_download(pool, paths, &row.dir_id, &row.name, row.size).await {
      tracing::warn!(event = "file_delete_local_failed", file_id = row.id.as_str(), error = %e, "Не удалось удалить локальный файл");
    }
    sqlx::query("DELETE FROM files WHERE id = ?")
      .bind(&row.id)
      .execute(pool)
      .await?;
  }
  Ok(())
}

pub async fn download_file(
  pool: &SqlitePool,
  tg: &dyn TelegramService,
  paths: &Paths,
  storage_chat_id: ChatId,
  file_id: &str,
  overwrite: bool
) -> anyhow::Result<PathBuf> {
  let row = sqlx::query("SELECT id, dir_id, name, size, tg_chat_id, tg_msg_id FROM files WHERE id = ?")
    .bind(file_id)
    .fetch_optional(pool)
    .await?;
  let Some(row) = row else {
    return Err(anyhow::anyhow!("Файл не найден"));
  };
  let dir_id: String = row.get("dir_id");
  let name: String = row.get("name");
  let size: i64 = row.get("size");
  let mut msg_chat_id: i64 = row.get("tg_chat_id");
  let mut msg_id: i64 = row.get("tg_msg_id");

  let dir_path = build_dir_path(pool, &dir_id).await?;
  let base_dir = paths.cache_dir.join("downloads").join(&dir_path);
  std::fs::create_dir_all(&base_dir)?;
  let existing = find_local_download(paths, &dir_path, &name, size);
  if let Some(existing_path) = existing.clone() {
    if !overwrite {
      return Ok(existing_path);
    }
  }
  let target_path = if overwrite {
    existing.unwrap_or_else(|| preferred_target_path(&base_dir, &name))
  } else {
    resolve_target_path(&base_dir, &name, size)?
  };
  if overwrite && target_path.exists() {
    let _ = std::fs::remove_file(&target_path);
  }

  if let Ok(path) = tg.download_message_file(msg_chat_id, msg_id, target_path.clone()).await {
    update_file_size_from_local(pool, file_id, &path).await?;
    return Ok(path);
  }

  if let Ok(Some((found_chat_id, found_msg_id))) =
    find_file_message(tg, msg_chat_id, storage_chat_id, file_id).await
  {
    if found_chat_id != msg_chat_id || found_msg_id != msg_id {
      msg_chat_id = found_chat_id;
      msg_id = found_msg_id;
      sqlx::query("UPDATE files SET tg_chat_id = ?, tg_msg_id = ?, is_broken = 0 WHERE id = ?")
        .bind(msg_chat_id)
        .bind(msg_id)
        .bind(file_id)
        .execute(pool)
        .await?;
    }
  }

  let path = tg.download_message_file(msg_chat_id, msg_id, target_path.clone()).await?;
  update_file_size_from_local(pool, file_id, &path).await?;
  Ok(path)
}

pub async fn find_local_download_path(pool: &SqlitePool, paths: &Paths, file_id: &str) -> anyhow::Result<Option<PathBuf>> {
  let row = sqlx::query("SELECT dir_id, name, size FROM files WHERE id = ?")
    .bind(file_id)
    .fetch_optional(pool)
    .await?;
  let Some(row) = row else {
    return Err(anyhow::anyhow!("Файл не найден"));
  };
  let dir_id: String = row.get("dir_id");
  let name: String = row.get("name");
  let size: i64 = row.get("size");
  let dir_path = build_dir_path(pool, &dir_id).await?;
  Ok(find_local_download(paths, &dir_path, &name, size))
}

pub async fn repair_file(
  pool: &SqlitePool,
  tg: &dyn TelegramService,
  paths: &Paths,
  storage_chat_id: ChatId,
  file_id: &str,
  upload_path: Option<&Path>
) -> anyhow::Result<RepairFileResult> {
  let row = sqlx::query("SELECT id, dir_id, name, size, hash, tg_chat_id, tg_msg_id FROM files WHERE id = ?")
    .bind(file_id)
    .fetch_optional(pool)
    .await?;
  let Some(row) = row else {
    return Err(anyhow::anyhow!("Файл не найден"));
  };

  let dir_id: String = row.get("dir_id");
  let name: String = row.get("name");
  let size: i64 = row.get("size");
  let hash: String = row.get("hash");
  let mut msg_chat_id: i64 = row.get("tg_chat_id");
  let mut msg_id: i64 = row.get("tg_msg_id");
  let dir_name = fetch_dir_name(pool, &dir_id).await?;
  let caption = make_file_caption_with_tag(
    &FileMeta {
      dir_id: dir_id.clone(),
      file_id: file_id.to_string(),
      name: name.clone(),
      hash_short: hash.clone()
    },
    dir_name.as_deref()
  );

  if tg.edit_message_caption(msg_chat_id, msg_id, caption.clone()).await.is_ok() {
    sqlx::query("UPDATE files SET tg_chat_id = ?, tg_msg_id = ?, is_broken = 0 WHERE id = ?")
      .bind(msg_chat_id)
      .bind(msg_id)
      .bind(file_id)
      .execute(pool)
      .await?;
    return Ok(RepairFileResult::Repaired);
  }

  if let Ok(Some((found_chat_id, found_msg_id))) =
    find_file_message(tg, msg_chat_id, storage_chat_id, file_id).await
  {
    msg_chat_id = found_chat_id;
    msg_id = found_msg_id;
    if tg.edit_message_caption(msg_chat_id, msg_id, caption.clone()).await.is_ok() {
      sqlx::query("UPDATE files SET tg_chat_id = ?, tg_msg_id = ?, is_broken = 0 WHERE id = ?")
        .bind(msg_chat_id)
        .bind(msg_id)
        .bind(file_id)
        .execute(pool)
        .await?;
      return Ok(RepairFileResult::Repaired);
    }
  }

  let source_path = if let Some(p) = upload_path {
    Some(p.to_path_buf())
  } else {
    let dir_path = build_dir_path(pool, &dir_id).await?;
    find_local_download(paths, &dir_path, &name, size)
  };

  let Some(source_path) = source_path else {
    return Ok(RepairFileResult::NeedFile);
  };
  if !source_path.is_file() {
    return Err(anyhow::anyhow!("Файл не найден"));
  }

  let uploaded = tg.send_file(storage_chat_id, source_path, caption).await?;
  sqlx::query("UPDATE files SET tg_chat_id = ?, tg_msg_id = ?, is_broken = 0 WHERE id = ?")
    .bind(uploaded.chat_id)
    .bind(uploaded.message_id)
    .bind(file_id)
    .execute(pool)
    .await?;

  Ok(RepairFileResult::Repaired)
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
    } else if (ch == '_' || ch.is_whitespace() || ch == '-' || ch == '.') && !last_underscore {
      out.push('_');
      last_underscore = true;
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

async fn build_dir_path(pool: &SqlitePool, dir_id: &str) -> anyhow::Result<PathBuf> {
  let mut names: Vec<String> = Vec::new();
  let mut current = Some(dir_id.to_string());
  let mut guard = 0;
  while let Some(id) = current {
    guard += 1;
    if guard > 64 {
      break;
    }
    let row = sqlx::query("SELECT name, parent_id FROM directories WHERE id = ?")
      .bind(&id)
      .fetch_optional(pool)
      .await?;
    let Some(row) = row else {
      break;
    };
    let name: String = row.get("name");
    if name.trim().is_empty() || name == "Неизвестная папка" {
      // пропускаем
    } else {
      names.push(sanitize_component(&name));
    }
    let parent = row.try_get::<String,_>("parent_id").ok();
    current = parent
      .filter(|p| !p.trim().is_empty() && p != "ROOT");
  }
  names.reverse();
  let mut path = PathBuf::new();
  for n in names {
    if !n.is_empty() {
      path.push(n);
    }
  }
  Ok(path)
}

fn sanitize_component(name: &str) -> String {
  let mut out = String::new();
  for ch in name.chars() {
    if ch == '/' || ch == '\\' || ch == ':' || ch == '\0' || ch.is_control() {
      out.push('_');
    } else {
      out.push(ch);
    }
  }
  let cleaned = out.trim().to_string();
  // Не допускаем спец-сегменты пути, чтобы исключить выход за пределы cache/downloads.
  if cleaned == "." || cleaned == ".." {
    "_".to_string()
  } else {
    cleaned
  }
}

fn resolve_target_path(base_dir: &Path, name: &str, size: i64) -> anyhow::Result<PathBuf> {
  let mut safe = sanitize_component(name);
  if safe.is_empty() {
    safe = "файл".to_string();
  }
  let mut candidate = base_dir.join(&safe);
  if candidate.exists() {
    if size > 0 {
      if let Ok(meta) = std::fs::metadata(&candidate) {
        if meta.len() as i64 == size {
          return Ok(candidate);
        }
      }
    }
    let (stem, ext) = split_name(&safe);
    for i in 1..=99 {
      let next = format!("{stem} ({i}){ext}");
      candidate = base_dir.join(&next);
      if !candidate.exists() {
        break;
      }
    }
  }
  Ok(candidate)
}

fn preferred_target_path(base_dir: &Path, name: &str) -> PathBuf {
  let mut safe = sanitize_component(name);
  if safe.is_empty() {
    safe = "файл".to_string();
  }
  base_dir.join(safe)
}

fn split_name(name: &str) -> (String, String) {
  if let Some(pos) = name.rfind('.') {
    if pos > 0 {
      let (a, b) = name.split_at(pos);
      return (a.to_string(), b.to_string());
    }
  }
  (name.to_string(), String::new())
}

fn is_name_variant(base_stem: &str, candidate_stem: &str) -> bool {
  if candidate_stem == base_stem {
    return true;
  }
  let Some(rest) = candidate_stem.strip_prefix(base_stem) else {
    return false;
  };
  let Some(rest) = rest.strip_prefix(" (") else {
    return false;
  };
  let Some(num) = rest.strip_suffix(")") else {
    return false;
  };
  !num.is_empty() && num.chars().all(|c| c.is_ascii_digit())
}

fn find_local_download(paths: &Paths, dir_path: &Path, name: &str, size: i64) -> Option<PathBuf> {
  let base_dir = paths.cache_dir.join("downloads").join(dir_path);
  if !base_dir.exists() {
    return None;
  }

  let mut safe = sanitize_component(name);
  if safe.is_empty() {
    safe = "файл".to_string();
  }
  let (stem, ext) = split_name(&safe);

  let entries = std::fs::read_dir(&base_dir).ok()?;
  let mut first_match: Option<PathBuf> = None;
  for entry in entries.flatten() {
    let path = entry.path();
    if !path.is_file() {
      continue;
    }
    let file_name = path.file_name().and_then(|n| n.to_str());
    let Some(file_name) = file_name else { continue; };
    let (cand_stem, cand_ext) = split_name(file_name);
    if cand_ext != ext || !is_name_variant(&stem, &cand_stem) {
      continue;
    }
    if first_match.is_none() {
      first_match = Some(path.clone());
    }
    if size > 0 {
      if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() as i64 != size {
          continue;
        }
      }
    }
    return Some(path);
  }
  first_match
}

fn local_download_info(paths: &Paths, dir_path: &Path, name: &str, size: i64) -> (bool, Option<i64>) {
  let Some(path) = find_local_download(paths, dir_path, name, size) else {
    return (false, None);
  };
  let local_size = std::fs::metadata(&path)
    .ok()
    .map(|meta| meta.len().min(i64::MAX as u64) as i64);
  (true, local_size)
}

async fn update_file_size_from_local(pool: &SqlitePool, file_id: &str, path: &Path) -> anyhow::Result<()> {
  let local_size = std::fs::metadata(path)
    .map(|meta| meta.len().min(i64::MAX as u64) as i64)
    .unwrap_or(0);
  if local_size <= 0 {
    return Ok(());
  }
  sqlx::query("UPDATE files SET size = ? WHERE id = ?")
    .bind(local_size)
    .bind(file_id)
    .execute(pool)
    .await?;
  Ok(())
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

async fn remove_local_download(
  pool: &SqlitePool,
  paths: &Paths,
  dir_id: &str,
  name: &str,
  _size: i64
) -> anyhow::Result<()> {
  let dir_path = build_dir_path(pool, dir_id).await?;
  let base_dir = paths.cache_dir.join("downloads").join(dir_path);
  if !base_dir.exists() {
    return Ok(());
  }

  let mut safe = sanitize_component(name);
  if safe.is_empty() {
    safe = "файл".to_string();
  }
  let (stem, ext) = split_name(&safe);
  let mut removed = false;

  let entries = match std::fs::read_dir(&base_dir) {
    Ok(v) => v,
    Err(_) => return Ok(())
  };
  for entry in entries.flatten() {
    let path = entry.path();
    if !path.is_file() {
      continue;
    }
    let file_name = match path.file_name().and_then(|n| n.to_str()) {
      Some(v) => v,
      None => continue
    };
    let (cand_stem, cand_ext) = split_name(file_name);
    if cand_ext != ext || !is_name_variant(&stem, &cand_stem) {
      continue;
    }
    if std::fs::remove_file(&path).is_ok() {
      removed = true;
    }
  }

  let should_cleanup = removed || std::fs::read_dir(&base_dir).map(|mut it| it.next().is_none()).unwrap_or(false);
  if should_cleanup {
    cleanup_empty_dirs(paths.cache_dir.join("downloads"), Some(&base_dir));
  }
  Ok(())
}

fn cleanup_empty_dirs(root: PathBuf, start: Option<&Path>) {
  let mut current = start.map(|p| p.to_path_buf());
  while let Some(dir) = current {
    if !dir.starts_with(&root) || dir == root {
      break;
    }
    let is_empty = std::fs::read_dir(&dir).map(|mut it| it.next().is_none()).unwrap_or(false);
    if !is_empty {
      break;
    }
    let parent = dir.parent().map(|p| p.to_path_buf());
    let _ = std::fs::remove_dir(&dir);
    current = parent;
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::collections::HashMap;
  use std::io::Write;
  use std::sync::{Arc, Mutex};
  use tempfile::tempdir;
  use crate::db::Db;
  use crate::sqlx;
  use crate::telegram::{
    ChatId,
    ChatInfo,
    HistoryMessage,
    MessageId,
    SearchMessagesResult,
    TelegramService,
    TgError,
    UploadedMessage
  };

  #[derive(Clone, Default)]
  struct MockTelegram {
    state: Arc<Mutex<MockTelegramState>>
  }

  #[derive(Default)]
  struct MockTelegramState {
    download_attempts: Vec<(ChatId, MessageId, PathBuf)>,
    fail_once_for: Option<(ChatId, MessageId)>,
    failed_once: bool,
    download_payloads: HashMap<(ChatId, MessageId), Vec<u8>>,
    search_results: HashMap<(ChatId, String, MessageId), SearchMessagesResult>
  }

  impl MockTelegram {
    fn with_payload(self, chat_id: ChatId, message_id: MessageId, payload: &[u8]) -> Self {
      let mut guard = self.state.lock().expect("mock lock");
      guard.download_payloads.insert((chat_id, message_id), payload.to_vec());
      drop(guard);
      self
    }

    fn fail_once(self, chat_id: ChatId, message_id: MessageId) -> Self {
      let mut guard = self.state.lock().expect("mock lock");
      guard.fail_once_for = Some((chat_id, message_id));
      drop(guard);
      self
    }

    fn with_search_result(
      self,
      chat_id: ChatId,
      query: String,
      from_message_id: MessageId,
      result: SearchMessagesResult
    ) -> Self {
      let mut guard = self.state.lock().expect("mock lock");
      guard.search_results.insert((chat_id, query, from_message_id), result);
      drop(guard);
      self
    }

    fn download_attempts(&self) -> Vec<(ChatId, MessageId)> {
      let guard = self.state.lock().expect("mock lock");
      guard
        .download_attempts
        .iter()
        .map(|(chat_id, message_id, _)| (*chat_id, *message_id))
        .collect()
    }
  }

  #[async_trait::async_trait]
  impl TelegramService for MockTelegram {
    async fn auth_start(&self, _phone: String) -> Result<(), TgError> {
      Err(TgError::NotImplemented)
    }

    async fn auth_submit_code(&self, _code: String) -> Result<(), TgError> {
      Err(TgError::NotImplemented)
    }

    async fn auth_submit_password(&self, _password: String) -> Result<(), TgError> {
      Err(TgError::NotImplemented)
    }

    async fn configure(&self, _api_id: i32, _api_hash: String, _tdlib_path: Option<String>) -> Result<(), TgError> {
      Err(TgError::NotImplemented)
    }

    async fn storage_check_channel(&self, _chat_id: ChatId) -> Result<bool, TgError> {
      Ok(false)
    }

    async fn storage_get_or_create_channel(&self) -> Result<ChatId, TgError> {
      Err(TgError::NotImplemented)
    }

    async fn storage_create_channel(&self) -> Result<ChatId, TgError> {
      Err(TgError::NotImplemented)
    }

    async fn storage_delete_channel(&self, _chat_id: ChatId) -> Result<(), TgError> {
      Err(TgError::NotImplemented)
    }

    async fn backup_check_channel(&self, _chat_id: ChatId) -> Result<bool, TgError> {
      Ok(false)
    }

    async fn backup_get_or_create_channel(&self) -> Result<ChatId, TgError> {
      Err(TgError::NotImplemented)
    }

    async fn chat_history(
      &self,
      _chat_id: ChatId,
      _from_message_id: MessageId,
      _limit: i32
    ) -> Result<SearchMessagesResult, TgError> {
      Err(TgError::NotImplemented)
    }

    async fn search_chat_messages(
      &self,
      chat_id: ChatId,
      query: String,
      from_message_id: MessageId,
      _limit: i32
    ) -> Result<SearchMessagesResult, TgError> {
      let guard = self.state.lock().expect("mock lock");
      Ok(guard
        .search_results
        .get(&(chat_id, query, from_message_id))
        .cloned()
        .unwrap_or(SearchMessagesResult {
          total_count: Some(0),
          next_from_message_id: 0,
          messages: Vec::new()
        }))
    }

    async fn search_storage_messages(
      &self,
      _chat_id: ChatId,
      _from_message_id: MessageId,
      _limit: i32
    ) -> Result<SearchMessagesResult, TgError> {
      Err(TgError::NotImplemented)
    }

    async fn search_chats(&self, _query: String, _limit: i32) -> Result<Vec<ChatInfo>, TgError> {
      Err(TgError::NotImplemented)
    }

    async fn recent_chats(&self, _limit: i32) -> Result<Vec<ChatInfo>, TgError> {
      Err(TgError::NotImplemented)
    }

    async fn send_text_message(&self, _chat_id: ChatId, _text: String) -> Result<UploadedMessage, TgError> {
      Err(TgError::NotImplemented)
    }

    async fn send_dir_message(&self, _chat_id: ChatId, _text: String) -> Result<UploadedMessage, TgError> {
      Err(TgError::NotImplemented)
    }

    async fn edit_message_text(&self, _chat_id: ChatId, _message_id: MessageId, _text: String) -> Result<(), TgError> {
      Err(TgError::NotImplemented)
    }

    async fn edit_message_caption(
      &self,
      _chat_id: ChatId,
      _message_id: MessageId,
      _caption: String
    ) -> Result<(), TgError> {
      Err(TgError::NotImplemented)
    }

    async fn send_file(
      &self,
      _chat_id: ChatId,
      _path: PathBuf,
      _caption: String
    ) -> Result<UploadedMessage, TgError> {
      Err(TgError::NotImplemented)
    }

    async fn send_file_from_message(
      &self,
      _chat_id: ChatId,
      _message_id: MessageId,
      _caption: String
    ) -> Result<UploadedMessage, TgError> {
      Err(TgError::NotImplemented)
    }

    async fn forward_message(
      &self,
      _from_chat_id: ChatId,
      _to_chat_id: ChatId,
      _message_id: MessageId
    ) -> Result<MessageId, TgError> {
      Err(TgError::NotImplemented)
    }

    async fn copy_messages(
      &self,
      _from_chat_id: ChatId,
      _to_chat_id: ChatId,
      _message_ids: Vec<MessageId>
    ) -> Result<Vec<Option<MessageId>>, TgError> {
      Err(TgError::NotImplemented)
    }

    async fn delete_messages(
      &self,
      _chat_id: ChatId,
      _message_ids: Vec<MessageId>,
      _revoke: bool
    ) -> Result<(), TgError> {
      Err(TgError::NotImplemented)
    }

    async fn download_message_file(
      &self,
      chat_id: ChatId,
      message_id: MessageId,
      target: PathBuf
    ) -> Result<PathBuf, TgError> {
      let mut guard = self.state.lock().expect("mock lock");
      guard.download_attempts.push((chat_id, message_id, target.clone()));
      if guard.fail_once_for == Some((chat_id, message_id)) && !guard.failed_once {
        guard.failed_once = true;
        return Err(TgError::Other("download failed".to_string()));
      }
      let Some(payload) = guard.download_payloads.get(&(chat_id, message_id)).cloned() else {
        return Err(TgError::Other("no payload rule".to_string()));
      };
      drop(guard);

      if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(TgError::Io)?;
      }
      std::fs::write(&target, payload).map_err(TgError::Io)?;
      Ok(target)
    }

    async fn message_exists(&self, _chat_id: ChatId, _message_id: MessageId) -> Result<bool, TgError> {
      Ok(false)
    }
  }

  async fn setup_db_and_paths() -> anyhow::Result<(tempfile::TempDir, Db, Paths)> {
    let tmp = tempdir()?;
    let paths = Paths::from_base(tmp.path().to_path_buf());
    paths.ensure_dirs()?;
    let db = Db::connect(tmp.path().join("test.sqlite")).await?;
    db.migrate().await?;
    Ok((tmp, db, paths))
  }

  async fn seed_one_file(
    pool: &SqlitePool,
    file_id: &str,
    dir_id: &str,
    name: &str,
    size: i64,
    tg_chat_id: i64,
    tg_msg_id: i64
  ) -> anyhow::Result<()> {
    sqlx::query(
      "INSERT INTO directories(id, parent_id, name, tg_msg_id, updated_at, is_broken) VALUES(?, NULL, ?, NULL, 0, 0)"
    )
      .bind(dir_id)
      .bind("Документы")
      .execute(pool)
      .await?;

    sqlx::query(
      "INSERT INTO files(id, dir_id, name, size, hash, tg_chat_id, tg_msg_id, created_at, is_broken)
       VALUES(?, ?, ?, ?, ?, ?, ?, 0, 0)"
    )
      .bind(file_id)
      .bind(dir_id)
      .bind(name)
      .bind(size)
      .bind("deadbeef")
      .bind(tg_chat_id)
      .bind(tg_msg_id)
      .execute(pool)
      .await?;

    Ok(())
  }

  #[test]
  fn find_local_download_returns_existing_when_size_unknown() {
    let tmp = tempdir().expect("tempdir");
    let paths = Paths::from_base(tmp.path().to_path_buf());
    let dir_path = PathBuf::from("docs");
    let base_dir = paths.cache_dir.join("downloads").join(&dir_path);
    std::fs::create_dir_all(&base_dir).expect("create dirs");
    let file_path = base_dir.join("report.txt");
    let mut file = std::fs::File::create(&file_path).expect("create file");
    writeln!(file, "hello").expect("write");

    let found = find_local_download(&paths, &dir_path, "report.txt", 0);
    assert_eq!(found, Some(file_path));
  }

  #[test]
  fn find_local_download_falls_back_when_size_mismatch() {
    let tmp = tempdir().expect("tempdir");
    let paths = Paths::from_base(tmp.path().to_path_buf());
    let dir_path = PathBuf::from("docs");
    let base_dir = paths.cache_dir.join("downloads").join(&dir_path);
    std::fs::create_dir_all(&base_dir).expect("create dirs");
    let file_path = base_dir.join("report.txt");
    let mut file = std::fs::File::create(&file_path).expect("create file");
    writeln!(file, "hello").expect("write");

    // Размер в БД мог устареть, но локальную копию все равно нужно переиспользовать.
    let found = find_local_download(&paths, &dir_path, "report.txt", 1024);
    assert_eq!(found, Some(file_path));
  }

  #[test]
  fn local_download_info_returns_actual_local_size() {
    let tmp = tempdir().expect("tempdir");
    let paths = Paths::from_base(tmp.path().to_path_buf());
    let dir_path = PathBuf::from("docs");
    let base_dir = paths.cache_dir.join("downloads").join(&dir_path);
    std::fs::create_dir_all(&base_dir).expect("create dirs");
    let file_path = base_dir.join("report.txt");
    std::fs::write(&file_path, b"hello world").expect("write");

    let (downloaded, local_size) = local_download_info(&paths, &dir_path, "report.txt", 0);
    assert!(downloaded);
    assert_eq!(local_size, Some(11));
  }

  #[test]
  fn sanitize_component_rejects_relative_segments() {
    assert_eq!(sanitize_component("."), "_");
    assert_eq!(sanitize_component(".."), "_");
    assert_eq!(sanitize_component("  ..  "), "_");
  }

  #[test]
  fn sanitize_component_replaces_forbidden_chars() {
    assert_eq!(sanitize_component("a/b\\c:d"), "a_b_c_d");
    assert_eq!(sanitize_component("name\0with\rbad\nchars"), "name_with_bad_chars");
  }

  #[tokio::test]
  async fn download_file_returns_existing_without_redownload_when_overwrite_disabled() -> anyhow::Result<()> {
    let (_tmp, db, paths) = setup_db_and_paths().await?;
    seed_one_file(db.pool(), "f1", "d1", "report.txt", 0, -1001, 100).await?;

    let existing_dir = paths.cache_dir.join("downloads").join("Документы");
    std::fs::create_dir_all(&existing_dir)?;
    let existing_path = existing_dir.join("report.txt");
    std::fs::write(&existing_path, b"cached")?;

    let tg = MockTelegram::default();
    let out = download_file(db.pool(), &tg, &paths, -2002, "f1", false).await?;

    assert_eq!(out, existing_path);
    assert_eq!(tg.download_attempts().len(), 0);
    Ok(())
  }

  #[tokio::test]
  async fn download_file_overwrite_redownloads_and_updates_size() -> anyhow::Result<()> {
    let (_tmp, db, paths) = setup_db_and_paths().await?;
    seed_one_file(db.pool(), "f2", "d2", "video.mp4", 6, -3001, 200).await?;

    let existing_dir = paths.cache_dir.join("downloads").join("Документы");
    std::fs::create_dir_all(&existing_dir)?;
    let existing_path = existing_dir.join("video.mp4");
    std::fs::write(&existing_path, b"oldold")?;

    let tg = MockTelegram::default().with_payload(-3001, 200, b"new payload bytes");
    let out = download_file(db.pool(), &tg, &paths, -3001, "f2", true).await?;

    assert_eq!(out, existing_path);
    assert_eq!(std::fs::read(&out)?, b"new payload bytes");
    assert_eq!(tg.download_attempts(), vec![(-3001, 200)]);

    let row = sqlx::query("SELECT size FROM files WHERE id = ?")
      .bind("f2")
      .fetch_one(db.pool())
      .await?;
    let size: i64 = row.get("size");
    assert_eq!(size, "new payload bytes".len() as i64);
    Ok(())
  }

  #[tokio::test]
  async fn download_file_fallback_finds_new_message_and_updates_db() -> anyhow::Result<()> {
    let (_tmp, db, paths) = setup_db_and_paths().await?;
    let file_id = "f3";
    seed_one_file(db.pool(), file_id, "d3", "archive.zip", 0, -4001, 300).await?;

    let caption = make_file_caption(&FileMeta {
      dir_id: "d3".to_string(),
      file_id: file_id.to_string(),
      name: "archive.zip".to_string(),
      hash_short: "deadbeef".to_string()
    });
    let query = format!("f={file_id}");
    let search_hit = SearchMessagesResult {
      total_count: Some(1),
      next_from_message_id: 0,
      messages: vec![HistoryMessage {
        id: 555,
        date: 0,
        text: None,
        caption: Some(caption),
        file_size: Some(0),
        file_name: Some("archive.zip".to_string())
      }]
    };

    let tg = MockTelegram::default()
      .fail_once(-4001, 300)
      .with_payload(-5002, 555, b"fallback payload")
      .with_search_result(-5002, query, 0, search_hit);

    let out = download_file(db.pool(), &tg, &paths, -5002, file_id, false).await?;
    assert_eq!(std::fs::read(&out)?, b"fallback payload");
    assert_eq!(tg.download_attempts(), vec![(-4001, 300), (-5002, 555)]);

    let row = sqlx::query("SELECT tg_chat_id, tg_msg_id, size FROM files WHERE id = ?")
      .bind(file_id)
      .fetch_one(db.pool())
      .await?;
    let chat_id: i64 = row.get("tg_chat_id");
    let msg_id: i64 = row.get("tg_msg_id");
    let size: i64 = row.get("size");
    assert_eq!(chat_id, -5002);
    assert_eq!(msg_id, 555);
    assert_eq!(size, "fallback payload".len() as i64);
    Ok(())
  }

  #[tokio::test]
  async fn delete_file_removes_local_copy_even_when_db_size_is_stale() -> anyhow::Result<()> {
    let (_tmp, db, paths) = setup_db_and_paths().await?;
    seed_one_file(db.pool(), "f_del", "d_del", "report.txt", 9999, -7001, 701).await?;

    let local_dir = paths.cache_dir.join("downloads").join("Документы");
    std::fs::create_dir_all(&local_dir)?;
    let local_path = local_dir.join("report.txt");
    std::fs::write(&local_path, b"local copy")?;

    let tg = MockTelegram::default();
    delete_file(db.pool(), &tg, &paths, "f_del").await?;

    assert!(!local_path.exists());

    let row = sqlx::query("SELECT id FROM files WHERE id = ?")
      .bind("f_del")
      .fetch_optional(db.pool())
      .await?;
    assert!(row.is_none());
    Ok(())
  }
}
