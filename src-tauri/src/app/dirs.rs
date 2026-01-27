use chrono::Utc;
use sqlx::{SqlitePool, Row};
use ulid::Ulid;

use crate::fsmeta::{DirMeta, make_dir_message};
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

  sqlx::query("UPDATE directories SET tg_msg_id = ?, updated_at = ? WHERE id = ?")
    .bind(uploaded.message_id)
    .bind(updated_at)
    .bind(&id)
    .execute(pool)
    .await?;

  Ok(id)
}

pub async fn list_tree(pool: &SqlitePool) -> anyhow::Result<DirNode> {
  let rows = sqlx::query("SELECT id, parent_id, name FROM directories ORDER BY name")
    .fetch_all(pool)
    .await?;

  #[derive(Clone)]
  struct RowItem { id: String, parent_id: Option<String>, name: String }

  let mut items: Vec<RowItem> = Vec::with_capacity(rows.len());
  for r in rows {
    items.push(RowItem {
      id: r.get::<String,_>("id"),
      parent_id: r.try_get::<String,_>("parent_id").ok(),
      name: r.get::<String,_>("name")
    });
  }

  let mut map: std::collections::HashMap<String, DirNode> = std::collections::HashMap::new();
  for it in &items {
    map.insert(it.id.clone(), DirNode { id: it.id.clone(), name: it.name.clone(), parent_id: it.parent_id.clone(), children: vec![] });
  }

  let mut root = DirNode { id: "ROOT".to_string(), name: "ROOT".to_string(), parent_id: None, children: vec![] };

  for it in &items {
    if let Some(pid) = &it.parent_id {
      if let (Some(parent), Some(child)) = (map.get_mut(pid), map.get(&it.id).cloned()) {
        parent.children.push(child);
      }
    } else {
      if let Some(child) = map.get(&it.id).cloned() {
        root.children.push(child);
      }
    }
  }

  fn rebuild(node: &DirNode, map: &std::collections::HashMap<String, DirNode>) -> DirNode {
    let mut n = node.clone();
    n.children = n.children.iter().map(|c| {
      let full = map.get(&c.id).cloned().unwrap_or_else(|| c.clone());
      rebuild(&full, map)
    }).collect();
    n
  }

  let rebuilt_root = DirNode {
    children: root.children.iter().map(|c| rebuild(c, &map)).collect(),
    ..root
  };

  Ok(rebuilt_root)
}
