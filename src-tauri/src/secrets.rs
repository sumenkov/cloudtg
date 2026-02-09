use std::path::{Path, PathBuf};

use anyhow::Context;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use chacha20poly1305::{aead::Aead, KeyInit, XChaCha20Poly1305, XNonce};
use getrandom::fill as getrandom_fill;
use serde::{Deserialize, Serialize};

use crate::paths::Paths;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TgCredentials {
  pub api_id: i32,
  pub api_hash: String
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CredentialsSource {
  Runtime,
  Keychain,
  EncryptedFile,
  Env
}

impl CredentialsSource {
  pub fn as_str(&self) -> &'static str {
    match self {
      CredentialsSource::Runtime => "runtime",
      CredentialsSource::Keychain => "keychain",
      CredentialsSource::EncryptedFile => "encrypted",
      CredentialsSource::Env => "env"
    }
  }
}

#[derive(Clone, Debug)]
pub struct CredentialsStatus {
  pub available: bool,
  pub source: Option<CredentialsSource>,
  pub keychain_available: bool,
  pub encrypted_present: bool,
  pub locked: bool
}

const KEYCHAIN_SERVICE: &str = "cloudtg";
const KEYCHAIN_ACCOUNT: &str = "tdlib_api";

#[derive(Serialize, Deserialize)]
struct EncryptedPayload {
  v: u8,
  salt: String,
  nonce: String,
  ciphertext: String
}

pub fn env_credentials() -> Option<TgCredentials> {
  let api_id = env_api_id();
  let api_hash = env_api_hash();
  match (api_id, api_hash) {
    (Some(id), Some(hash)) => Some(TgCredentials { api_id: id, api_hash: hash }),
    _ => None
  }
}

pub fn normalize_credentials(api_id: i32, api_hash: String) -> anyhow::Result<TgCredentials> {
  if api_id <= 0 {
    return Err(anyhow::anyhow!("Некорректный API_ID"));
  }
  let hash = api_hash.trim().to_string();
  if hash.is_empty() {
    return Err(anyhow::anyhow!("Некорректный API_HASH"));
  }
  Ok(TgCredentials { api_id, api_hash: hash })
}

pub fn resolve_credentials(paths: &Paths, runtime: Option<&TgCredentials>) -> (Option<TgCredentials>, CredentialsStatus) {
  let encrypted_present = encrypted_exists(paths);
  let mut status = CredentialsStatus {
    available: false,
    source: None,
    keychain_available: true,
    encrypted_present,
    locked: false
  };

  if let Some(creds) = runtime {
    status.available = true;
    status.source = Some(CredentialsSource::Runtime);
    return (Some(creds.clone()), status);
  }

  match keychain_get() {
    Ok(Some(creds)) => {
      status.available = true;
      status.source = Some(CredentialsSource::Keychain);
      return (Some(creds), status);
    }
    Ok(None) => {}
    Err(_) => {
      status.keychain_available = false;
    }
  }

  if let Some(creds) = env_credentials() {
    status.available = true;
    status.source = Some(CredentialsSource::Env);
    return (Some(creds), status);
  }

  if encrypted_present {
    status.locked = true;
  }

  (None, status)
}

pub fn encrypted_path(paths: &Paths) -> PathBuf {
  paths.data_dir.join("secrets").join("tg_keys.enc.json")
}

pub fn encrypted_exists(paths: &Paths) -> bool {
  encrypted_path(paths).exists()
}

pub fn encrypted_save(paths: &Paths, creds: &TgCredentials, password: &str) -> anyhow::Result<()> {
  if password.trim().is_empty() {
    return Err(anyhow::anyhow!("Нужен пароль для шифрования"));
  }
  let payload = encrypt_payload(creds, password)?;
  let path = encrypted_path(paths);
  if let Some(parent) = path.parent() {
    std::fs::create_dir_all(parent)?;
  }
  write_atomic(&path, &payload)?;
  Ok(())
}

pub fn encrypted_load(paths: &Paths, password: &str) -> anyhow::Result<TgCredentials> {
  let path = encrypted_path(paths);
  let data = std::fs::read(&path).with_context(|| "Не удалось прочитать зашифрованные ключи")?;
  decrypt_payload(&data, password)
}

pub fn encrypted_clear(paths: &Paths) -> anyhow::Result<()> {
  let path = encrypted_path(paths);
  if path.exists() {
    std::fs::remove_file(path)?;
  }
  Ok(())
}

pub fn keychain_get() -> anyhow::Result<Option<TgCredentials>> {
  let entry = keychain_entry()?;
  match entry.get_password() {
    Ok(secret) => {
      let creds: TgCredentials = serde_json::from_str(&secret)
        .map_err(|e| anyhow::anyhow!("Некорректные данные в системном хранилище: {e}"))?;
      Ok(Some(creds))
    }
    Err(err) => {
      if is_keychain_missing(&err) {
        Ok(None)
      } else {
        Err(anyhow::anyhow!("Системное хранилище недоступно: {err}"))
      }
    }
  }
}

pub fn keychain_set(creds: &TgCredentials) -> anyhow::Result<()> {
  let entry = keychain_entry()?;
  let payload = serde_json::to_string(creds)?;
  entry.set_password(&payload).map_err(|e| anyhow::anyhow!("Не удалось сохранить ключи в системном хранилище: {e}"))?;
  Ok(())
}

pub fn keychain_clear() -> anyhow::Result<()> {
  let entry = keychain_entry()?;
  match entry.delete_credential() {
    Ok(_) => Ok(()),
    Err(err) => {
      if is_keychain_missing(&err) {
        Ok(())
      } else {
        Err(anyhow::anyhow!("Не удалось удалить ключи из системного хранилища: {err}"))
      }
    }
  }
}

fn keychain_entry() -> anyhow::Result<keyring::Entry> {
  #[allow(clippy::redundant_closure)]
  keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)
    .map_err(|e| anyhow::anyhow!("Не удалось инициализировать системное хранилище: {e}"))
}

fn is_keychain_missing(err: &keyring::Error) -> bool {
  let msg = err.to_string().to_lowercase();
  msg.contains("not found") || msg.contains("no entry") || msg.contains("no such")
}

fn env_api_id() -> Option<i32> {
  let raw = std::env::var("CLOUDTG_API_ID").ok()
    .or_else(|| option_env!("CLOUDTG_API_ID").map(|v| v.to_string()));
  let id = raw.and_then(|v| v.parse::<i32>().ok())?;
  if id > 0 { Some(id) } else { None }
}

fn env_api_hash() -> Option<String> {
  let raw = std::env::var("CLOUDTG_API_HASH").ok()
    .or_else(|| option_env!("CLOUDTG_API_HASH").map(|v| v.to_string()));
  let hash = raw?.trim().to_string();
  if hash.is_empty() { None } else { Some(hash) }
}

fn encrypt_payload(creds: &TgCredentials, password: &str) -> anyhow::Result<Vec<u8>> {
  let payload = serde_json::to_vec(creds)?;
  let mut salt = [0u8; 16];
  getrandom_fill(&mut salt).map_err(|e| anyhow::anyhow!("Не удалось получить случайные байты: {e}"))?;
  let mut key = [0u8; 32];
  argon2::Argon2::default()
    .hash_password_into(password.as_bytes(), &salt, &mut key)
    .map_err(|e| anyhow::anyhow!("Не удалось создать ключ шифрования: {e}"))?;
  let cipher = XChaCha20Poly1305::new((&key).into());
  let mut nonce = [0u8; 24];
  getrandom_fill(&mut nonce).map_err(|e| anyhow::anyhow!("Не удалось получить случайные байты: {e}"))?;
  let ciphertext = cipher.encrypt(XNonce::from_slice(&nonce), payload.as_ref())
    .map_err(|_| anyhow::anyhow!("Не удалось зашифровать ключи"))?;

  let sealed = EncryptedPayload {
    v: 1,
    salt: BASE64.encode(salt),
    nonce: BASE64.encode(nonce),
    ciphertext: BASE64.encode(ciphertext)
  };

  serde_json::to_vec(&sealed).map_err(|e| anyhow::anyhow!("Не удалось сериализовать ключи: {e}"))
}

fn decrypt_payload(data: &[u8], password: &str) -> anyhow::Result<TgCredentials> {
  let sealed: EncryptedPayload = serde_json::from_slice(data)
    .map_err(|e| anyhow::anyhow!("Некорректный формат зашифрованных ключей: {e}"))?;
  if sealed.v != 1 {
    return Err(anyhow::anyhow!("Неподдерживаемая версия зашифрованных ключей"));
  }

  let salt = BASE64.decode(sealed.salt.as_bytes())
    .map_err(|_| anyhow::anyhow!("Некорректная соль в зашифрованных ключах"))?;
  let nonce = BASE64.decode(sealed.nonce.as_bytes())
    .map_err(|_| anyhow::anyhow!("Некорректный nonce в зашифрованных ключах"))?;
  let ciphertext = BASE64.decode(sealed.ciphertext.as_bytes())
    .map_err(|_| anyhow::anyhow!("Некорректный ciphertext в зашифрованных ключах"))?;
  if nonce.len() != 24 {
    return Err(anyhow::anyhow!("Некорректная длина nonce в зашифрованных ключах"));
  }

  let mut key = [0u8; 32];
  argon2::Argon2::default()
    .hash_password_into(password.as_bytes(), &salt, &mut key)
    .map_err(|e| anyhow::anyhow!("Не удалось создать ключ шифрования: {e}"))?;

  let cipher = XChaCha20Poly1305::new((&key).into());
  let plain = cipher.decrypt(XNonce::from_slice(&nonce), ciphertext.as_ref())
    .map_err(|_| anyhow::anyhow!("Неверный пароль или поврежденные данные"))?;
  let creds: TgCredentials = serde_json::from_slice(&plain)
    .map_err(|e| anyhow::anyhow!("Некорректные данные ключей: {e}"))?;
  Ok(creds)
}

fn write_atomic(path: &Path, data: &[u8]) -> anyhow::Result<()> {
  let tmp = path.with_extension("tmp");
  std::fs::write(&tmp, data)?;
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
  }
  std::fs::rename(&tmp, path)?;
  Ok(())
}
