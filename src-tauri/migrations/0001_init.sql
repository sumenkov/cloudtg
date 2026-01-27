CREATE TABLE IF NOT EXISTS directories (
  id TEXT PRIMARY KEY NOT NULL,
  parent_id TEXT NULL,
  name TEXT NOT NULL,
  tg_msg_id INTEGER NULL,
  updated_at INTEGER NOT NULL,
  FOREIGN KEY(parent_id) REFERENCES directories(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_directories_parent ON directories(parent_id);

CREATE TABLE IF NOT EXISTS files (
  id TEXT PRIMARY KEY NOT NULL,
  dir_id TEXT NOT NULL,
  name TEXT NOT NULL,
  size INTEGER NOT NULL,
  hash TEXT NOT NULL,
  tg_chat_id INTEGER NOT NULL,
  tg_msg_id INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  FOREIGN KEY(dir_id) REFERENCES directories(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_files_dir ON files(dir_id);
CREATE INDEX IF NOT EXISTS idx_files_name ON files(name);

CREATE TABLE IF NOT EXISTS sync_state (
  key TEXT PRIMARY KEY NOT NULL,
  value TEXT NOT NULL
);
