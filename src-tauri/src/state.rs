use std::sync::Arc;

use parking_lot::RwLock;
use tauri::AppHandle;

use crate::{paths::Paths, db::Db, telegram::{TelegramService, make_telegram_service}};

#[derive(Clone)]
pub struct AppState {
  inner: Arc<RwLock<Inner>>,
}

struct Inner {
  paths: Option<Paths>,
  db: Option<Db>,
  telegram: Option<Arc<dyn TelegramService>>,
  auth_state: AuthState
}

#[derive(Clone, Debug, serde::Serialize)]
pub enum AuthState {
  Unknown,
  NeedsAuth,
  Ready
}

impl AppState {
  pub fn new() -> Self {
    Self {
      inner: Arc::new(RwLock::new(Inner {
        paths: None,
        db: None,
        telegram: None,
        auth_state: AuthState::Unknown
      }))
    }
  }

  pub fn auth_state(&self) -> AuthState {
    self.inner.read().auth_state.clone()
  }

  pub fn set_auth_state(&self, s: AuthState) {
    self.inner.write().auth_state = s;
  }

  pub fn db(&self) -> anyhow::Result<Db> {
    self.inner.read().db.clone().ok_or_else(|| anyhow::anyhow!("DB not initialized yet"))
  }

  pub fn telegram(&self) -> anyhow::Result<Arc<dyn TelegramService>> {
    self.inner.read().telegram.clone().ok_or_else(|| anyhow::anyhow!("Telegram service not initialized yet"))
  }

  pub fn paths(&self) -> anyhow::Result<Paths> {
    self.inner.read().paths.clone().ok_or_else(|| anyhow::anyhow!("Paths not initialized yet"))
  }

  pub fn spawn_init(&self, app: AppHandle) {
    let state = self.clone();
    tauri::async_runtime::spawn(async move {
      if let Err(e) = state.init(app).await {
        tracing::error!(error = %e, "init failed");
      }
    });
  }

  async fn init(&self, app: AppHandle) -> anyhow::Result<()> {
    let paths = Paths::detect()?;
    paths.ensure_dirs()?;

    let db = Db::connect(paths.sqlite_path()).await?;
    db.migrate().await?;

    let telegram = make_telegram_service(paths.clone(), app.clone())?;

    {
      let mut w = self.inner.write();
      w.paths = Some(paths);
      w.db = Some(db);
      w.telegram = Some(telegram);
      // если mock_telegram включён, считаем, что "авторизовано"
      w.auth_state = if cfg!(feature = "mock_telegram") { AuthState::Ready } else { AuthState::NeedsAuth };
    }

    Ok(())
  }
}
