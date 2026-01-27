use std::collections::HashMap;

pub const TAG_PREFIX: &str = "#ocltg #v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMeta {
  pub dir_id: String,
  pub file_id: String,
  pub name: String,
  pub hash_short: String
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirMeta {
  pub dir_id: String,
  pub parent_id: String, // "ROOT" or ULID
  pub name: String
}

#[derive(thiserror::Error, Debug)]
pub enum MetaError {
  #[error("not a cloudtg message")]
  NotCloudtg,
  #[error("missing field: {0}")]
  Missing(&'static str)
}

fn kv_map(input: &str) -> HashMap<String, String> {
  input
    .split_whitespace()
    .filter_map(|t| t.split_once('='))
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

pub fn make_file_caption(m: &FileMeta) -> String {
  format!("{TAG_PREFIX} #file d={} f={} n={} h={}",
    m.dir_id, m.file_id, escape_spaces(&m.name), m.hash_short
  )
}

pub fn make_dir_message(m: &DirMeta) -> String {
  format!("{TAG_PREFIX} #dir d={} p={} name={}",
    m.dir_id, m.parent_id, escape_spaces(&m.name)
  )
}

pub fn parse_file_caption(caption: &str) -> Result<FileMeta, MetaError> {
  if !caption.contains("#ocltg") || !caption.contains("#v1") || !caption.contains("#file") {
    return Err(MetaError::NotCloudtg);
  }
  let map = kv_map(caption);
  Ok(FileMeta {
    dir_id: map.get("d").cloned().ok_or(MetaError::Missing("d"))?,
    file_id: map.get("f").cloned().ok_or(MetaError::Missing("f"))?,
    name: unescape_spaces(map.get("n").cloned().ok_or(MetaError::Missing("n"))?.as_str()),
    hash_short: map.get("h").cloned().ok_or(MetaError::Missing("h"))?
  })
}

pub fn parse_dir_message(text: &str) -> Result<DirMeta, MetaError> {
  if !text.contains("#ocltg") || !text.contains("#v1") || !text.contains("#dir") {
    return Err(MetaError::NotCloudtg);
  }
  let map = kv_map(text);
  Ok(DirMeta {
    dir_id: map.get("d").cloned().ok_or(MetaError::Missing("d"))?,
    parent_id: map.get("p").cloned().ok_or(MetaError::Missing("p"))?,
    name: unescape_spaces(map.get("name").cloned().ok_or(MetaError::Missing("name"))?.as_str())
  })
}

// Replace spaces with underscores, escape underscore itself.
fn escape_spaces(s: &str) -> String {
  s.replace('_', "__").replace(' ', "_")
}
fn unescape_spaces(s: &str) -> String {
  let placeholder = "\u{0000}";
  let tmp = s.replace("__", placeholder);
  let tmp = tmp.replace('_', " ");
  tmp.replace(placeholder, "_")
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn file_roundtrip() {
    let m = FileMeta {
      dir_id: "01HAAA".into(),
      file_id: "01HBBB".into(),
      name: "report final_v2.pdf".into(),
      hash_short: "1a2b3c4d".into()
    };
    let cap = make_file_caption(&m);
    let parsed = parse_file_caption(&cap).unwrap();
    assert_eq!(parsed, m);
  }

  #[test]
  fn dir_roundtrip() {
    let m = DirMeta { dir_id: "01HCCC".into(), parent_id: "ROOT".into(), name: "My Projects".into() };
    let txt = make_dir_message(&m);
    let parsed = parse_dir_message(&txt).unwrap();
    assert_eq!(parsed, m);
  }
}
