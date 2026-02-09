use std::path::PathBuf;

use crate::sqlx::migrate::Migrator;
use sqlx_sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool};

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
    // Используем runtime-мигратор без macro-фичи sqlx.
    let migrations_path = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/migrations"));
    let migrator = Migrator::new(migrations_path).await?;
    migrator.run(&self.pool).await?;
    Ok(())
  }
}
