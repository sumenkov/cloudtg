use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Paths {
  pub base_dir: PathBuf,
  pub data_dir: PathBuf,
  pub cache_dir: PathBuf,
  pub logs_dir: PathBuf
}

impl Paths {
  pub fn detect() -> anyhow::Result<Self> {
    if let Ok(p) = std::env::var("CLOUDTG_BASE_DIR") {
      return Ok(Self::from_base(PathBuf::from(p)));
    }
    let exe = std::env::current_exe()?;
    let base_dir = exe.parent().unwrap_or_else(|| Path::new(".")).to_path_buf();
    Ok(Self::from_base(base_dir))
  }

  pub fn from_base(base_dir: PathBuf) -> Self {
    let data_dir = base_dir.join("data");
    let cache_dir = base_dir.join("cache");
    let logs_dir = base_dir.join("logs");
    Self { base_dir, data_dir, cache_dir, logs_dir }
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
