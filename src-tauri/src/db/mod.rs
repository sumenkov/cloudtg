use std::path::PathBuf;

use ::sqlx::migrate::Migrator;
use sqlx_sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool};

static MIGRATOR: Migrator = sqlx_macros::migrate!("./migrations");

#[derive(Clone)]
pub struct Db {
  pool: SqlitePool
}

impl Db {
  pub async fn connect(path: PathBuf) -> anyhow::Result<Self> {
    let opts = SqliteConnectOptions::new()
      .filename(path)
      .create_if_missing(true)
      .journal_mode(SqliteJournalMode::Wal);

    let pool = SqlitePool::connect_with(opts).await?;
    Ok(Self { pool })
  }

  pub fn pool(&self) -> &SqlitePool {
    &self.pool
  }

  pub async fn migrate(&self) -> anyhow::Result<()> {
    // Миграции встроены в бинарник на этапе сборки.
    MIGRATOR.run(&self.pool).await?;
    Ok(())
  }
}
