use std::path::PathBuf;

use sqlx::{SqlitePool, sqlite::SqliteConnectOptions};
use sqlx::migrate::Migrator;

// NOTE: use the default migrations directory resolution (CARGO_MANIFEST_DIR/migrations)
// to avoid the "paths relative to the current file's directory are not currently supported" error.
static MIGRATOR: Migrator = sqlx::migrate!();

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
