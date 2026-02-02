use std::collections::HashSet;

use chrono::Utc;
use sqlx::{SqlitePool, Row};

use crate::telegram::{TelegramService, ChatId, HistoryMessage};
use crate::app::{indexer, sync};

#[derive(Debug, Clone, serde::Serialize)]
pub struct ReconcileOutcome {
  pub scanned: i64,
  pub dir_seen: i64,
  pub file_seen: i64,
  pub imported: i64,
  pub marked_dirs: i64,
  pub marked_files: i64,
  pub cleared_dirs: i64,
  pub cleared_files: i64,
  pub min_message_id: i64,
  pub max_message_id: i64
}

pub async fn reconcile_recent(
  pool: &SqlitePool,
  tg: &dyn TelegramService,
  storage_chat_id: ChatId,
  limit: i64
) -> anyhow::Result<ReconcileOutcome> {
  let limit = limit.max(1);
  let messages = fetch_recent_messages(tg, storage_chat_id, limit).await?;
  if messages.is_empty() {
    return Ok(ReconcileOutcome {
      scanned: 0,
      dir_seen: 0,
      file_seen: 0,
      imported: 0,
      marked_dirs: 0,
      marked_files: 0,
      cleared_dirs: 0,
      cleared_files: 0,
      min_message_id: 0,
      max_message_id: 0
    });
  }

  let mut seen_dirs: HashSet<i64> = HashSet::new();
  let mut seen_files: HashSet<i64> = HashSet::new();
  let mut dir_seen = 0;
  let mut file_seen = 0;
  let mut imported = 0;
  let mut unassigned_dir: Option<(String, String)> = None;

  for msg in &messages {
    let outcome = indexer::index_storage_message(pool, tg, storage_chat_id, msg, &mut unassigned_dir).await?;
    if outcome.dir {
      seen_dirs.insert(msg.id);
      dir_seen += 1;
    }
    if outcome.file {
      seen_files.insert(msg.id);
      file_seen += 1;
    }
    if outcome.imported {
      imported += 1;
    }
  }

  let min_id = messages.iter().map(|m| m.id).min().unwrap_or(0);
  let max_id = messages.iter().map(|m| m.id).max().unwrap_or(0);

  let (marked_dirs, cleared_dirs) = if min_id > 0 {
    mark_broken_dirs(pool, min_id, &seen_dirs).await?
  } else {
    (0, 0)
  };
  let (marked_files, cleared_files) = if min_id > 0 {
    mark_broken_files(pool, storage_chat_id, min_id, &seen_files).await?
  } else {
    (0, 0)
  };

  if max_id > 0 {
    let current = sync::get_sync(pool, "storage_last_message_id")
      .await?
      .and_then(|v| v.parse::<i64>().ok())
      .unwrap_or(0);
    if max_id > current {
      sync::set_sync(pool, "storage_last_message_id", &max_id.to_string()).await?;
    }
  }
  let _ = sync::set_sync(pool, "storage_reconcile_done", &Utc::now().to_rfc3339()).await;

  Ok(ReconcileOutcome {
    scanned: messages.len() as i64,
    dir_seen,
    file_seen,
    imported,
    marked_dirs,
    marked_files,
    cleared_dirs,
    cleared_files,
    min_message_id: min_id,
    max_message_id: max_id
  })
}

async fn fetch_recent_messages(
  tg: &dyn TelegramService,
  chat_id: ChatId,
  limit: i64
) -> anyhow::Result<Vec<HistoryMessage>> {
  let mut out: Vec<HistoryMessage> = Vec::new();
  let mut from_message_id: i64 = 0;
  let mut remaining: i64 = limit.max(1);

  while remaining > 0 {
    let batch = tg.chat_history(chat_id, from_message_id, remaining.min(100) as i32).await?;
    if batch.messages.is_empty() {
      break;
    }
    for msg in batch.messages {
      out.push(msg);
      remaining -= 1;
      if remaining <= 0 {
        break;
      }
    }

    if remaining <= 0 {
      break;
    }
    if batch.next_from_message_id == 0 || batch.next_from_message_id == from_message_id {
      break;
    }
    from_message_id = batch.next_from_message_id;
  }

  Ok(out)
}

async fn mark_broken_dirs(
  pool: &SqlitePool,
  min_message_id: i64,
  seen: &HashSet<i64>
) -> anyhow::Result<(i64, i64)> {
  let rows = sqlx::query(
    "SELECT id, tg_msg_id, is_broken FROM directories WHERE tg_msg_id IS NOT NULL AND tg_msg_id >= ?"
  )
    .bind(min_message_id)
    .fetch_all(pool)
    .await?;

  let mut marked = 0i64;
  let mut cleared = 0i64;

  for row in rows {
    let id: String = row.get("id");
    let msg_id: i64 = row.get("tg_msg_id");
    let is_broken: i64 = row.get("is_broken");
    if msg_id <= 0 {
      continue;
    }
    let should_broken = !seen.contains(&msg_id);
    if should_broken && is_broken == 0 {
      sqlx::query("UPDATE directories SET is_broken = 1 WHERE id = ?")
        .bind(&id)
        .execute(pool)
        .await?;
      marked += 1;
    } else if !should_broken && is_broken != 0 {
      sqlx::query("UPDATE directories SET is_broken = 0 WHERE id = ?")
        .bind(&id)
        .execute(pool)
        .await?;
      cleared += 1;
    }
  }

  Ok((marked, cleared))
}

async fn mark_broken_files(
  pool: &SqlitePool,
  storage_chat_id: ChatId,
  min_message_id: i64,
  seen: &HashSet<i64>
) -> anyhow::Result<(i64, i64)> {
  let rows = sqlx::query(
    "SELECT id, tg_msg_id, is_broken FROM files WHERE tg_chat_id = ? AND tg_msg_id >= ?"
  )
    .bind(storage_chat_id)
    .bind(min_message_id)
    .fetch_all(pool)
    .await?;

  let mut marked = 0i64;
  let mut cleared = 0i64;

  for row in rows {
    let id: String = row.get("id");
    let msg_id: i64 = row.get("tg_msg_id");
    let is_broken: i64 = row.get("is_broken");
    if msg_id <= 0 {
      continue;
    }
    let should_broken = !seen.contains(&msg_id);
    if should_broken && is_broken == 0 {
      sqlx::query("UPDATE files SET is_broken = 1 WHERE id = ?")
        .bind(&id)
        .execute(pool)
        .await?;
      marked += 1;
    } else if !should_broken && is_broken != 0 {
      sqlx::query("UPDATE files SET is_broken = 0 WHERE id = ?")
        .bind(&id)
        .execute(pool)
        .await?;
      cleared += 1;
    }
  }

  Ok((marked, cleared))
}
