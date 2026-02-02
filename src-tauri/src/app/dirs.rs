use chrono::Utc;
use sqlx::{SqlitePool, Row};
use ulid::Ulid;

use crate::fsmeta::{DirMeta, make_dir_message, parse_dir_message};
use crate::telegram::{TelegramService, ChatId};

use super::models::DirNode;

pub async fn create_dir(
  pool: &SqlitePool,
  tg: &dyn TelegramService,
  chat_id: ChatId,
  parent_id: Option<String>,
  name: String
) -> anyhow::Result<String> {
  let id = Ulid::new().to_string();
  let updated_at = Utc::now().timestamp();
  let parent_id = parent_id.filter(|p| !p.trim().is_empty() && p != "ROOT");

  sqlx::query("INSERT INTO directories(id, parent_id, name, tg_msg_id, updated_at) VALUES(?, ?, ?, NULL, ?)")
    .bind(&id)
    .bind(parent_id.as_deref())
    .bind(&name)
    .bind(updated_at)
    .execute(pool)
    .await?;

  let parent_tag = parent_id.clone().unwrap_or_else(|| "ROOT".to_string());
  let msg = make_dir_message(&DirMeta { dir_id: id.clone(), parent_id: parent_tag, name });
  let uploaded = tg.send_dir_message(chat_id, msg).await?;

  sqlx::query("UPDATE directories SET tg_msg_id = ?, updated_at = ?, is_broken = 0 WHERE id = ?")
    .bind(uploaded.message_id)
    .bind(updated_at)
    .bind(&id)
    .execute(pool)
    .await?;

  Ok(id)
}

pub async fn rename_dir(
  pool: &SqlitePool,
  tg: &dyn TelegramService,
  chat_id: ChatId,
  dir_id: &str,
  name: String
) -> anyhow::Result<()> {
  let name = name.trim().to_string();
  if name.is_empty() {
    return Err(anyhow::anyhow!("Имя папки не может быть пустым"));
  }
  let mut dir = fetch_dir(pool, dir_id).await?;
  if dir.name == name && dir.tg_msg_id.is_some() {
    return Ok(());
  }
  let msg_id = ensure_dir_message(tg, chat_id, &dir, dir.parent_id.clone(), &name).await?;
  let updated_at = Utc::now().timestamp();
  sqlx::query("UPDATE directories SET name = ?, tg_msg_id = ?, updated_at = ?, is_broken = 0 WHERE id = ?")
    .bind(&name)
    .bind(msg_id)
    .bind(updated_at)
    .bind(dir_id)
    .execute(pool)
    .await?;
  dir.name = name;
  dir.tg_msg_id = Some(msg_id);
  Ok(())
}

pub async fn move_dir(
  pool: &SqlitePool,
  tg: &dyn TelegramService,
  chat_id: ChatId,
  dir_id: &str,
  parent_id: Option<String>
) -> anyhow::Result<()> {
  let mut dir = fetch_dir(pool, dir_id).await?;
  let parent_id = normalize_parent_id(parent_id);

  if let Some(pid) = parent_id.as_deref() {
    if pid == dir_id {
      return Err(anyhow::anyhow!("Нельзя переместить папку внутрь самой себя"));
    }
    if !dir_exists(pool, pid).await? {
      return Err(anyhow::anyhow!("Родительская папка не найдена"));
    }
    if has_ancestor(pool, pid, dir_id).await? {
      return Err(anyhow::anyhow!("Нельзя переместить папку в ее подпапку"));
    }
  }

  if dir.parent_id == parent_id && dir.tg_msg_id.is_some() {
    return Ok(());
  }
  let msg_id = ensure_dir_message(tg, chat_id, &dir, parent_id.clone(), &dir.name).await?;
  let updated_at = Utc::now().timestamp();
  sqlx::query("UPDATE directories SET parent_id = ?, tg_msg_id = ?, updated_at = ?, is_broken = 0 WHERE id = ?")
    .bind(parent_id.as_deref())
    .bind(msg_id)
    .bind(updated_at)
    .bind(dir_id)
    .execute(pool)
    .await?;
  dir.parent_id = parent_id;
  dir.tg_msg_id = Some(msg_id);
  Ok(())
}

pub async fn delete_dir(
  pool: &SqlitePool,
  tg: &dyn TelegramService,
  chat_id: ChatId,
  dir_id: &str
) -> anyhow::Result<()> {
  let dir = fetch_dir(pool, dir_id).await?;
  let child_count: i64 = sqlx::query("SELECT COUNT(1) as cnt FROM directories WHERE parent_id = ?")
    .bind(dir_id)
    .fetch_one(pool)
    .await?
    .get::<i64,_>("cnt");
  let file_count: i64 = sqlx::query("SELECT COUNT(1) as cnt FROM files WHERE dir_id = ?")
    .bind(dir_id)
    .fetch_one(pool)
    .await?
    .get::<i64,_>("cnt");

  if child_count > 0 || file_count > 0 {
    return Err(anyhow::anyhow!(
      "Папка не пустая: файлов={file_count}, подпапок={child_count}"
    ));
  }

  let mut message_ids: Vec<i64> = Vec::new();
  if let Some(msg_id) = dir.tg_msg_id {
    message_ids.push(msg_id);
  }
  if let Ok(mut found) = find_dir_messages(tg, chat_id, dir_id).await {
    message_ids.append(&mut found);
  }
  message_ids.sort_unstable();
  message_ids.dedup();
  if !message_ids.is_empty() {
    if let Err(e) = tg.delete_messages(chat_id, message_ids, true).await {
      tracing::warn!(event = "dir_delete_message_failed", dir_id = dir_id, error = %e, "Не удалось удалить сообщение папки");
    }
  }

  sqlx::query("DELETE FROM directories WHERE id = ?")
    .bind(dir_id)
    .execute(pool)
    .await?;
  Ok(())
}

pub async fn list_tree(pool: &SqlitePool) -> anyhow::Result<DirNode> {
  let rows = sqlx::query("SELECT id, parent_id, name, is_broken FROM directories ORDER BY name")
    .fetch_all(pool)
    .await?;

  #[derive(Clone)]
  struct RowItem { id: String, parent_id: Option<String>, name: String, is_broken: bool }

  let mut items: Vec<RowItem> = Vec::with_capacity(rows.len());
  for r in rows {
    let raw_parent = r.try_get::<String,_>("parent_id").ok();
    let parent_id = raw_parent.filter(|p| !p.trim().is_empty() && p != "ROOT");
    items.push(RowItem {
      id: r.get::<String,_>("id"),
      parent_id,
      name: r.get::<String,_>("name"),
      is_broken: r.get::<i64,_>("is_broken") != 0
    });
  }

  let mut map: std::collections::HashMap<String, DirNode> = std::collections::HashMap::new();
  for it in &items {
    map.insert(
      it.id.clone(),
      DirNode {
        id: it.id.clone(),
        name: it.name.clone(),
        parent_id: it.parent_id.clone(),
        is_broken: it.is_broken,
        children: vec![]
      }
    );
  }

  let mut root = DirNode {
    id: "ROOT".to_string(),
    name: "ROOT".to_string(),
    parent_id: None,
    is_broken: false,
    children: vec![]
  };

  for it in &items {
    if let Some(pid) = &it.parent_id {
      // Avoid simultaneous mutable+immutable borrows of the same map.
      let child = map.get(&it.id).cloned();
      if let (Some(parent), Some(child)) = (map.get_mut(pid), child) {
        parent.children.push(child);
      }
    } else {
      if let Some(child) = map.get(&it.id).cloned() {
        root.children.push(child);
      }
    }
  }

  fn rebuild(node: &DirNode, map: &std::collections::HashMap<String, DirNode>) -> DirNode {
    let full = map.get(&node.id).cloned().unwrap_or_else(|| node.clone());
    let mut n = full.clone();
    n.children = full.children.iter().map(|c| rebuild(c, map)).collect();
    n
  }

  let rebuilt_root = DirNode {
    children: root.children.iter().map(|c| rebuild(c, &map)).collect(),
    ..root
  };

  Ok(rebuilt_root)
}

#[derive(Clone)]
struct DirRow {
  id: String,
  parent_id: Option<String>,
  name: String,
  tg_msg_id: Option<i64>
}

async fn fetch_dir(pool: &SqlitePool, dir_id: &str) -> anyhow::Result<DirRow> {
  let row = sqlx::query("SELECT id, parent_id, name, tg_msg_id FROM directories WHERE id = ?")
    .bind(dir_id)
    .fetch_optional(pool)
    .await?;
  let Some(row) = row else {
    return Err(anyhow::anyhow!("Папка не найдена"));
  };
  let parent_id = normalize_parent_id(row.try_get::<String,_>("parent_id").ok());
  Ok(DirRow {
    id: row.get::<String,_>("id"),
    parent_id,
    name: row.get::<String,_>("name"),
    tg_msg_id: row.try_get::<i64,_>("tg_msg_id").ok()
  })
}

fn normalize_parent_id(raw: Option<String>) -> Option<String> {
  raw.filter(|p| !p.trim().is_empty() && p != "ROOT")
}

pub(crate) async fn dir_exists(pool: &SqlitePool, dir_id: &str) -> anyhow::Result<bool> {
  let count: i64 = sqlx::query("SELECT COUNT(1) as cnt FROM directories WHERE id = ?")
    .bind(dir_id)
    .fetch_one(pool)
    .await?
    .get::<i64,_>("cnt");
  Ok(count > 0)
}

async fn has_ancestor(pool: &SqlitePool, start_id: &str, target_id: &str) -> anyhow::Result<bool> {
  let mut current: Option<String> = Some(start_id.to_string());
  while let Some(id) = current {
    if id == target_id {
      return Ok(true);
    }
    let row = sqlx::query("SELECT parent_id FROM directories WHERE id = ?")
      .bind(&id)
      .fetch_optional(pool)
      .await?;
    current = row
      .and_then(|r| r.try_get::<String,_>("parent_id").ok())
      .and_then(|p| normalize_parent_id(Some(p)));
  }
  Ok(false)
}

async fn ensure_dir_message(
  tg: &dyn TelegramService,
  chat_id: ChatId,
  dir: &DirRow,
  parent_id: Option<String>,
  name: &str
) -> anyhow::Result<i64> {
  let parent_tag = parent_id.unwrap_or_else(|| "ROOT".to_string());
  let msg = make_dir_message(&DirMeta { dir_id: dir.id.clone(), parent_id: parent_tag, name: name.to_string() });

  if let Some(msg_id) = dir.tg_msg_id {
    match tg.edit_message_text(chat_id, msg_id, msg.clone()).await {
      Ok(()) => return Ok(msg_id),
      Err(e) => {
        tracing::warn!(event = "dir_message_edit_failed", dir_id = dir.id, error = %e, "Не удалось обновить сообщение папки, отправляю новое");
        let uploaded = tg.send_dir_message(chat_id, msg).await?;
        let _ = tg.delete_messages(chat_id, vec![msg_id], true).await;
        return Ok(uploaded.message_id);
      }
    }
  }

  let uploaded = tg.send_dir_message(chat_id, msg).await?;
  Ok(uploaded.message_id)
}

async fn find_dir_messages(
  tg: &dyn TelegramService,
  chat_id: ChatId,
  dir_id: &str
) -> anyhow::Result<Vec<i64>> {
  let query = format!("d={dir_id}");
  let mut from_message_id: i64 = 0;
  let mut out = Vec::new();

  for _ in 0..8 {
    let batch = match tg.search_chat_messages(chat_id, query.clone(), from_message_id, 100).await {
      Ok(v) => v,
      Err(_) => break
    };
    for msg in batch.messages {
      if let Some(text) = msg.text.as_deref() {
        if let Ok(meta) = parse_dir_message(text) {
          if meta.dir_id == dir_id {
            out.push(msg.id);
          }
        }
      }
    }
    if batch.next_from_message_id == 0 {
      break;
    }
    from_message_id = batch.next_from_message_id;
  }

  Ok(out)
}
