#[derive(Debug, Clone, serde::Serialize)]
pub struct DirNode {
  pub id: String,
  pub name: String,
  pub parent_id: Option<String>,
  pub is_broken: bool,
  pub children: Vec<DirNode>
}
