use tempfile::tempdir;
use sqlx::Row;

#[tokio::test]
async fn migrations_apply_and_basic_insert_works() -> anyhow::Result<()> {
  let dir = tempdir()?;
  let db_path = dir.path().join("test.sqlite");

  let db = cloudtg_lib::db::Db::connect(db_path).await?;
  db.migrate().await?;

  let id = "01HTESTDIR";
  sqlx::query("INSERT INTO directories(id, parent_id, name, tg_msg_id, updated_at) VALUES(?, NULL, ?, NULL, 0)")
    .bind(id)
    .bind("Test")
    .execute(db.pool())
    .await?;

  let row = sqlx::query("SELECT name FROM directories WHERE id = ?")
    .bind(id)
    .fetch_one(db.pool())
    .await?;

  let name: String = row.get("name");
  assert_eq!(name, "Test");
  Ok(())
}
