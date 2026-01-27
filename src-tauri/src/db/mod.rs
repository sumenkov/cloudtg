use std::path::PathBuf;

use sqlx::{SqlitePool, sqlite::SqliteConnectOptions};
use sqlx::migrate::Migrator;

static MIGRATOR: Migrator = sqlx::migrate!("migrations");

#[derive(Clone)]
pub struct Db {
  pool: SqlitePool
}

impl Db {
  pub async fn connect(path: PathBuf) -> anyhow::Result<Self> {
    let opts = SqliteConnectOptions::new()
      .filename(path)
      .create_if_missing(true)
      .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);

    let pool = SqlitePool::connect_with(opts).await?;
    Ok(Self { pool })
  }

  pub fn pool(&self) -> &SqlitePool {
    &self.pool
  }

  pub async fn migrate(&self) -> anyhow::Result<()> {
    MIGRATOR.run(&self.pool).await?;
    Ok(())
  }
}
