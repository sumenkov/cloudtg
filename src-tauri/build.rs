fn main() {
  if let Ok(dir) = std::env::var("CARGO_MANIFEST_DIR") {
    let _ = std::env::set_current_dir(dir);
  }
  tauri_build::build();
}
