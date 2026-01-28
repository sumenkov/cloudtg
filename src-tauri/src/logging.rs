use std::path::PathBuf;

use once_cell::sync::OnceCell;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use crate::paths::Paths;

static LOG_GUARD: OnceCell<WorkerGuard> = OnceCell::new();

pub fn init() {
  let filter = EnvFilter::try_from_default_env()
    .unwrap_or_else(|_| EnvFilter::new("info,cloudtg=debug,cloudtg_lib=debug,tauri=info"));

  let logs_dir = detect_logs_dir();
  if logs_dir.is_none() {
    if cfg!(target_os = "windows") {
      eprintln!("Не удалось создать директорию для логов. Проверь права на запись.");
    } else {
      eprintln!(
        "Не удалось создать директорию для логов. Проверь права на запись (Linux/macOS: CLOUDTG_STORAGE_DIR)."
      );
    }
  }

  let stdout_layer = tracing_subscriber::fmt::layer()
    .with_target(true);

  let registry = tracing_subscriber::registry()
    .with(filter)
    .with(stdout_layer);

  if let Some(dir) = logs_dir {
    let file_appender = tracing_appender::rolling::daily(&dir, "cloudtg.jsonl");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    let _ = LOG_GUARD.set(guard);

    let file_layer = tracing_subscriber::fmt::layer()
      .json()
      .with_ansi(false)
      .with_target(true)
      .with_writer(non_blocking);
    registry.with(file_layer).init();
  } else {
    registry.init();
  }
}

fn detect_logs_dir() -> Option<PathBuf> {
  if let Ok(paths) = Paths::detect() {
    if std::fs::create_dir_all(&paths.logs_dir).is_ok() {
      return Some(paths.logs_dir);
    }
  }

  None
}
