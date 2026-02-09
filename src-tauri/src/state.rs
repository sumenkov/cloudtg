use std::sync::Arc;
use std::path::{Path, PathBuf};

use parking_lot::RwLock;
use tauri::{AppHandle, Manager};

use crate::{paths::Paths, db::Db, telegram::{TelegramService, make_telegram_service}, secrets::{TgCredentials, CredentialsSource}};

#[derive(Clone)]
pub struct AppState {
  inner: Arc<RwLock<Inner>>,
}

struct Inner {
  paths: Option<Paths>,
  db: Option<Db>,
  telegram: Option<Arc<dyn TelegramService>>,
  auth_state: AuthState,
  tg_credentials: Option<TgCredentials>,
  tg_credentials_source: Option<CredentialsSource>
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub enum AuthState {
  Unknown,
  WaitConfig,
  WaitPhone,
  WaitCode,
  WaitPassword,
  Ready,
  Closed
}

impl AppState {
  pub fn new() -> Self {
    Self {
      inner: Arc::new(RwLock::new(Inner {
        paths: None,
        db: None,
        telegram: None,
        auth_state: AuthState::Unknown,
        tg_credentials: None,
        tg_credentials_source: None
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
    self.inner.read().db.clone().ok_or_else(|| anyhow::anyhow!("База данных еще не инициализирована"))
  }

  pub fn telegram(&self) -> anyhow::Result<Arc<dyn TelegramService>> {
    self.inner.read().telegram.clone().ok_or_else(|| anyhow::anyhow!("Telegram сервис еще не инициализирован"))
  }

  pub fn paths(&self) -> anyhow::Result<Paths> {
    self.inner.read().paths.clone().ok_or_else(|| anyhow::anyhow!("Пути еще не инициализированы"))
  }

  pub fn tg_credentials(&self) -> Option<(TgCredentials, CredentialsSource)> {
    let inner = self.inner.read();
    inner.tg_credentials.clone().and_then(|creds| {
      inner.tg_credentials_source.map(|source| (creds, source))
    })
  }

  pub fn set_tg_credentials(&self, creds: TgCredentials, source: CredentialsSource) {
    let mut inner = self.inner.write();
    inner.tg_credentials = Some(creds);
    inner.tg_credentials_source = Some(source);
  }

  pub fn clear_tg_credentials(&self) {
    let mut inner = self.inner.write();
    inner.tg_credentials = None;
    inner.tg_credentials_source = None;
  }

  pub fn spawn_init(&self, app: AppHandle) {
    let state = self.clone();
    tauri::async_runtime::spawn(async move {
      if let Err(e) = state.init(app).await {
        tracing::error!(error = %e, "инициализация не удалась");
      }
    });
  }

  async fn init(&self, app: AppHandle) -> anyhow::Result<()> {
    let paths = Paths::detect()?.with_resource_dir(app.path().resource_dir().ok());
    paths.ensure_dirs()?;
    if let Err(e) = apply_pending_restore(&paths) {
      tracing::warn!(error = %e, "Не удалось применить подготовленное восстановление базы");
    }
    tracing::info!(event = "init_paths", base_dir = %paths.base_dir.display(), "Пути приложения инициализированы");

    let db = Db::connect(paths.sqlite_path()).await?;
    db.migrate().await?;
    tracing::info!(event = "init_db", db_path = %paths.sqlite_path().display(), "База данных подключена");

    let (tg_settings, _) = crate::secrets::resolve_credentials(&paths, None);
    let tdlib_path = crate::settings::get_tdlib_path(db.pool()).await?;
    let telegram = make_telegram_service(paths.clone(), app.clone(), tg_settings, tdlib_path)?;
    tracing::info!(event = "init_telegram_service", "Telegram сервис инициализирован");

    {
      let mut w = self.inner.write();
      w.paths = Some(paths);
      w.db = Some(db);
      w.telegram = Some(telegram);
      // если mock_telegram включён, считаем, что "авторизовано"
      w.auth_state = if cfg!(feature = "mock_telegram") { AuthState::Ready } else { AuthState::Unknown };
    }

    Ok(())
  }
}

impl Default for AppState {
  fn default() -> Self {
    Self::new()
  }
}

fn apply_pending_restore(paths: &Paths) -> anyhow::Result<()> {
  let pending = paths.pending_restore_path();
  if !pending.exists() {
    return Ok(());
  }

  let db_path = paths.sqlite_path();
  let prev_path = paths.previous_db_path();

  remove_sqlite_sidecars(&db_path);
  remove_sqlite_sidecars(&pending);
  remove_sqlite_sidecars(&prev_path);

  if db_path.exists() {
    if prev_path.exists() {
      let _ = std::fs::remove_file(&prev_path);
    }
    std::fs::rename(&db_path, &prev_path)?;
  }

  std::fs::rename(&pending, &db_path)?;
  remove_sqlite_sidecars(&prev_path);
  tracing::info!(
    event = "db_restore_applied",
    db_path = %db_path.display(),
    prev_path = %prev_path.display(),
    "Применено восстановление базы"
  );
  Ok(())
}

fn remove_sqlite_sidecars(path: &Path) {
  let base = path.to_string_lossy();
  for suffix in ["-wal", "-shm", "-journal"] {
    let candidate = PathBuf::from(format!("{base}{suffix}"));
    let _ = std::fs::remove_file(candidate);
  }
}
