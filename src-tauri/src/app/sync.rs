use sqlx::{SqlitePool, Row};

pub async fn get_sync(pool: &SqlitePool, key: &str) -> anyhow::Result<Option<String>> {
  let row = sqlx::query("SELECT value FROM sync_state WHERE key = ?")
    .bind(key)
    .fetch_optional(pool)
    .await?;
  Ok(row.map(|r| r.get::<String, _>("value")))
}

pub async fn set_sync(pool: &SqlitePool, key: &str, value: &str) -> anyhow::Result<()> {
  sqlx::query("INSERT INTO sync_state(key, value) VALUES(?, ?) ON CONFLICT(key) DO UPDATE SET value=excluded.value")
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;
  Ok(())
}
