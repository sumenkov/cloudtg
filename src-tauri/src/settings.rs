use sqlx::{SqlitePool, Row};

#[derive(Clone, serde::Serialize)]
pub struct TgSettings {
  pub api_id: i32,
  pub api_hash: String
}

#[derive(serde::Serialize)]
pub struct TgSettingsView {
  pub api_id: Option<i32>,
  pub api_hash: Option<String>,
  pub tdlib_path: Option<String>
}

pub async fn get_tg_settings(pool: &SqlitePool) -> anyhow::Result<Option<TgSettings>> {
  let api_id = get_value(pool, "tg_api_id").await?;
  let api_hash = get_value(pool, "tg_api_hash").await?;

  let api_id = match api_id {
    Some(v) => Some(v.parse::<i32>().map_err(|_| anyhow::anyhow!("Некорректный tg_api_id"))?),
    None => None
  };

  match (api_id, api_hash) {
    (Some(id), Some(hash)) => Ok(Some(TgSettings { api_id: id, api_hash: hash })),
    _ => Ok(None)
  }
}

pub async fn get_tdlib_path(pool: &SqlitePool) -> anyhow::Result<Option<String>> {
  get_value(pool, "tdlib_path").await
}

pub async fn get_tg_settings_view(pool: &SqlitePool) -> anyhow::Result<TgSettingsView> {
  let api_id = get_value(pool, "tg_api_id").await?;
  let api_hash = get_value(pool, "tg_api_hash").await?;
  let tdlib_path = get_value(pool, "tdlib_path").await?;

  let api_id = match api_id {
    Some(v) => Some(v.parse::<i32>().map_err(|_| anyhow::anyhow!("Некорректный tg_api_id"))?),
    None => None
  };

  Ok(TgSettingsView { api_id, api_hash, tdlib_path })
}

pub async fn set_tg_settings(
  pool: &SqlitePool,
  api_id: i32,
  api_hash: String,
  tdlib_path: Option<String>
) -> anyhow::Result<()> {
  set_value(pool, "tg_api_id", &api_id.to_string()).await?;
  set_value(pool, "tg_api_hash", &api_hash).await?;

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
