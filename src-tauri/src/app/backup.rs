use std::path::{Path, PathBuf};

use chrono::Utc;
use crate::sqlx;
use sqlx_sqlite::SqlitePool;

use crate::db::Db;
use crate::paths::Paths;
use crate::settings;
use crate::telegram::{ChatId, TelegramService};

use super::{indexer, sync};

pub const BACKUP_TAG: &str = "#ocltg #backup #v1";

#[derive(Debug, Default)]
pub struct RebuildStats {
  pub processed: i64,
  pub dirs: i64,
  pub files: i64,
  pub imported: i64,
  pub failed: i64
}

pub fn build_backup_caption(app_version: &str) -> String {
  let ts = Utc::now().to_rfc3339();
  format!("{BACKUP_TAG} ts={ts} app={app_version}")
}

pub async fn create_backup_snapshot(pool: &SqlitePool, paths: &Paths) -> anyhow::Result<PathBuf> {
  let dir = paths.backup_dir();
  std::fs::create_dir_all(&dir)?;
  let ts = Utc::now().format("%Y%m%d-%H%M%S");
  let file_path = dir.join(format!("cloudtg-backup-{ts}.sqlite"));

  let escaped = escape_sqlite_path(&file_path);
  let sql = format!("VACUUM INTO '{}'", escaped);
  sqlx::query(&sql).execute(pool).await?;

  Ok(file_path)
}

pub async fn rebuild_storage_to_path(
  target_path: &Path,
  tg: &dyn TelegramService,
  storage_chat_id: ChatId,
  tdlib_path: Option<&str>
) -> anyhow::Result<RebuildStats> {
  if let Some(parent) = target_path.parent() {
    std::fs::create_dir_all(parent)?;
  }
  if target_path.exists() {
    let _ = std::fs::remove_file(target_path);
  }

  let db = Db::connect(target_path.to_path_buf()).await?;
  db.migrate().await?;
  let pool = db.pool();

  sync::set_sync(pool, "storage_chat_id", &storage_chat_id.to_string()).await?;
  if let Some(p) = tdlib_path {
    settings::set_tdlib_path(pool, Some(p.to_string())).await?;
  }

  let mut from_message_id: i64 = 0;
  let mut newest_seen: Option<i64> = None;
  let mut stats = RebuildStats::default();
  let mut unassigned_dir: Option<(String, String)> = None;

  loop {
    let batch = tg.chat_history(storage_chat_id, from_message_id, 100).await?;
    if batch.messages.is_empty() {
      break;
    }

    for msg in batch.messages {
      stats.processed += 1;
      if newest_seen.is_none() {
        newest_seen = Some(msg.id);
      }
      let outcome = indexer::index_storage_message(pool, tg, storage_chat_id, &msg, &mut unassigned_dir).await?;
      if outcome.dir {
        stats.dirs += 1;
      }
      if outcome.file {
        stats.files += 1;
      }
      if outcome.imported {
        stats.imported += 1;
      }
      if outcome.failed {
        stats.failed += 1;
      }
    }

    if batch.next_from_message_id == 0 || batch.next_from_message_id == from_message_id {
      break;
    }
    from_message_id = batch.next_from_message_id;
  }

  if let Some(latest) = newest_seen {
    sync::set_sync(pool, "storage_last_message_id", &latest.to_string()).await?;
  }
  sync::set_sync(pool, "storage_sync_done", &Utc::now().to_rfc3339()).await?;

  Ok(stats)
}

fn escape_sqlite_path(path: &Path) -> String {
  path.to_string_lossy().replace('\'', "''")
}
