use std::env;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Paths {
  pub base_dir: PathBuf,
  pub data_dir: PathBuf,
  pub cache_dir: PathBuf,
  pub logs_dir: PathBuf,
  pub resource_dir: Option<PathBuf>
}

impl Paths {
  pub fn detect() -> anyhow::Result<Self> {
    let base_dir = if let Ok(p) = env::var("CLOUDTG_BASE_DIR") {
      PathBuf::from(p)
    } else {
      let exe = std::env::current_exe()?;
      exe.parent().unwrap_or_else(|| Path::new(".")).to_path_buf()
    };
    let mut paths = Self::from_base(base_dir);

    #[cfg(not(target_os = "windows"))]
    {
      let storage_dir = resolve_storage_dir(&paths.base_dir);
      paths.data_dir = storage_dir.join("data");
      paths.cache_dir = storage_dir.join("cache");
      paths.logs_dir = storage_dir.join("logs");
    }

    Ok(paths)
  }

  pub fn from_base(base_dir: PathBuf) -> Self {
    let data_dir = base_dir.join("data");
    let cache_dir = base_dir.join("cache");
    let logs_dir = base_dir.join("logs");
    Self {
      base_dir,
      data_dir,
      cache_dir,
      logs_dir,
      resource_dir: None
    }
  }

  pub fn with_resource_dir(mut self, resource_dir: Option<PathBuf>) -> Self {
    self.resource_dir = resource_dir;
    self
  }

  pub fn ensure_dirs(&self) -> anyhow::Result<()> {
    std::fs::create_dir_all(&self.data_dir)?;
    std::fs::create_dir_all(&self.cache_dir)?;
    std::fs::create_dir_all(&self.logs_dir)?;
    std::fs::create_dir_all(self.cache_dir.join("downloads"))?;
    Ok(())
  }

  pub fn sqlite_path(&self) -> PathBuf {
    self.data_dir.join("cloudtg.sqlite")
  }
}

#[cfg(not(target_os = "windows"))]
fn resolve_storage_dir(base_dir: &Path) -> PathBuf {
  if let Ok(p) = env::var("CLOUDTG_STORAGE_DIR") {
    let path = PathBuf::from(p);
    if can_use_storage(&path) {
      return path;
    }
    eprintln!("Не удалось создать директорию хранения по CLOUDTG_STORAGE_DIR. Укажи путь с доступными правами.");
  }

  if can_use_storage(base_dir) {
    return base_dir.to_path_buf();
  }

  if let Some(path) = user_storage_dir() {
    if can_use_storage(&path) {
      eprintln!("Нет прав на директорию хранения рядом с бинарём. Использую {path:?}. Можно задать CLOUDTG_STORAGE_DIR.");
      return path;
    }
  }

  eprintln!("Не удалось создать директорию хранения. Укажи CLOUDTG_STORAGE_DIR с доступным путём.");
  base_dir.to_path_buf()
}

#[cfg(not(target_os = "windows"))]
fn can_use_storage(dir: &Path) -> bool {
  if std::fs::create_dir_all(dir).is_err() {
    return false;
  }
  let probe = dir.join(".cloudtg_write_test");
  if std::fs::create_dir_all(&probe).is_ok() {
    let _ = std::fs::remove_dir_all(&probe);
    return true;
  }
  false
}

#[cfg(not(target_os = "windows"))]
fn user_storage_dir() -> Option<PathBuf> {
  if let Ok(p) = env::var("XDG_DATA_HOME") {
    return Some(PathBuf::from(p).join("cloudtg"));
  }
  #[cfg(target_os = "macos")]
  {
    if let Ok(home) = env::var("HOME") {
      return Some(PathBuf::from(home).join("Library").join("Application Support").join("CloudTG"));
    }
  }
  if let Ok(home) = env::var("HOME") {
    return Some(PathBuf::from(home).join(".local").join("share").join("cloudtg"));
  }
  None
}
