use sqlx::{SqlitePool, Row};

pub async fn get_tdlib_path(pool: &SqlitePool) -> anyhow::Result<Option<String>> {
  get_value(pool, "tdlib_path").await
}

pub async fn set_tdlib_path(pool: &SqlitePool, tdlib_path: Option<String>) -> anyhow::Result<()> {
  match tdlib_path {
    Some(p) if !p.trim().is_empty() => set_value(pool, "tdlib_path", p.trim()).await?,
    _ => clear_value(pool, "tdlib_path").await?
  }

  Ok(())
}

async fn get_value(pool: &SqlitePool, key: &str) -> anyhow::Result<Option<String>> {
  let row = sqlx::query("SELECT value FROM sync_state WHERE key = ?")
    .bind(key)
    .fetch_optional(pool)
    .await?;
  Ok(row.map(|r| r.get::<String, _>("value")))
}

async fn set_value(pool: &SqlitePool, key: &str, value: &str) -> anyhow::Result<()> {
  sqlx::query("INSERT INTO sync_state(key, value) VALUES(?, ?) ON CONFLICT(key) DO UPDATE SET value=excluded.value")
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;
  Ok(())
}

async fn clear_value(pool: &SqlitePool, key: &str) -> anyhow::Result<()> {
  sqlx::query("DELETE FROM sync_state WHERE key = ?")
    .bind(key)
    .execute(pool)
    .await?;
  Ok(())
}
