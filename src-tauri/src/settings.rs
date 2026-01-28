use sqlx::{SqlitePool, Row};
use std::env;

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
    _ => Ok(env_tg_settings())
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

  let mut api_id = api_id;
  let mut api_hash = api_hash;
  if api_id.is_none() || api_hash.is_none() {
    if let Some(env_settings) = env_tg_settings() {
      if api_id.is_none() {
        api_id = Some(env_settings.api_id);
      }
      if api_hash.is_none() {
        api_hash = Some(env_settings.api_hash);
      }
    }
  }

  Ok(TgSettingsView { api_id, api_hash, tdlib_path })
}

pub async fn set_tg_settings(
  pool: &SqlitePool,
  api_id: i32,
  api_hash: String,
  tdlib_path: Option<String>
) -> anyhow::Result<()> {
  let _ = api_id;
  let _ = api_hash;
  clear_value(pool, "tg_api_id").await?;
  clear_value(pool, "tg_api_hash").await?;

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

fn env_tg_settings() -> Option<TgSettings> {
  let api_id = env_api_id();
  let api_hash = env_api_hash();
  match (api_id, api_hash) {
    (Some(id), Some(hash)) => Some(TgSettings { api_id: id, api_hash: hash }),
    _ => None
  }
}

pub fn env_tg_settings_public() -> Option<TgSettings> {
  env_tg_settings()
}

fn env_api_id() -> Option<i32> {
  let raw = env::var("CLOUDTG_API_ID").ok()
    .or_else(|| option_env!("CLOUDTG_API_ID").map(|v| v.to_string()));
  let id = raw.and_then(|v| v.parse::<i32>().ok())?;
  if id > 0 { Some(id) } else { None }
}

fn env_api_hash() -> Option<String> {
  let raw = env::var("CLOUDTG_API_HASH").ok()
    .or_else(|| option_env!("CLOUDTG_API_HASH").map(|v| v.to_string()));
  let hash = raw?.trim().to_string();
  if hash.is_empty() { None } else { Some(hash) }
}
