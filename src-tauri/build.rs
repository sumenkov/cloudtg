fn main() {
  if let Ok(dir) = std::env::var("CARGO_MANIFEST_DIR") {
    let _ = std::env::set_current_dir(dir);
  }
  let _ = dotenvy::dotenv();
  let embed = std::env::var("CLOUDTG_EMBED_API_KEYS")
    .ok()
    .map(|v| matches!(v.trim().to_lowercase().as_str(), "1" | "true" | "yes"))
    .unwrap_or(false);
  if embed {
    if let Ok(v) = std::env::var("CLOUDTG_API_ID") {
      if !v.trim().is_empty() {
        println!("cargo:rustc-env=CLOUDTG_API_ID={}", v.trim());
      }
    }
    if let Ok(v) = std::env::var("CLOUDTG_API_HASH") {
      if !v.trim().is_empty() {
        println!("cargo:rustc-env=CLOUDTG_API_HASH={}", v.trim());
      }
    }
  }
  println!("cargo:rerun-if-env-changed=CLOUDTG_EMBED_API_KEYS");
  println!("cargo:rerun-if-env-changed=CLOUDTG_API_ID");
  println!("cargo:rerun-if-env-changed=CLOUDTG_API_HASH");
  tauri_build::build();
}
