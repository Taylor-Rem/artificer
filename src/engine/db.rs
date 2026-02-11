use std::sync::{Arc, Mutex, MutexGuard};
use anyhow::Result;
use rusqlite::Connection;
use serde_json::json;

#[derive(Clone)]
pub struct Db {
    db: Arc<Mutex<Connection>>,
}

impl Default for Db {
    fn default() -> Self {
        let db_path = dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("RustroverProjects")
            .join("artificer")
            .join("memory.db");

        if let Some(parent) = db_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let conn = Connection::open(&db_path).expect("Failed to open database");
        Self::create_tables(&conn).expect("Failed to create tables");

        Self {
            db: Arc::new(Mutex::new(conn)),
        }
    }
}

impl Db {
    pub fn query(&self, sql: &str, params: impl rusqlite::Params) -> Result<String> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(sql)?;
        let column_names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();

        let rows: Vec<serde_json::Value> = stmt
            .query_map(params, |row| {
                let mut map = serde_json::Map::new();
                for (i, name) in column_names.iter().enumerate() {
                    let val: rusqlite::types::Value = row.get(i)?;
                    let json_val = match val {
                        rusqlite::types::Value::Null => serde_json::Value::Null,
                        rusqlite::types::Value::Integer(n) => json!(n),
                        rusqlite::types::Value::Real(f) => json!(f),
                        rusqlite::types::Value::Text(s) => json!(s),
                        rusqlite::types::Value::Blob(b) => json!(format!("<blob:{} bytes>", b.len())),
                    };
                    map.insert(name.clone(), json_val);
                }
                Ok(serde_json::Value::Object(map))
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(json!(rows).to_string())
    }

    pub fn execute(&self, sql: &str, params: impl rusqlite::Params) -> Result<usize> {
        let conn = self.lock()?;
        Ok(conn.execute(sql, params)?)
    }

    pub fn lock(&self) -> Result<MutexGuard<'_, Connection>> {
        self.db.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))
    }

    fn create_tables(conn: &Connection) -> Result<()> {
        conn.execute_batch("
            CREATE TABLE IF NOT EXISTS conversation (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT,
                location TEXT NOT NULL,
                created INTEGER NOT NULL,
                last_accessed INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_title ON conversation(title);

            CREATE TABLE IF NOT EXISTS message (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                conversation_id INTEGER,
                role TEXT NOT NULL,
                message TEXT NOT NULL,
                \"order\" INTEGER NOT NULL,
                created INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_conversation_id ON message(conversation_id);

            CREATE TABLE IF NOT EXISTS jobs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                method TEXT NOT NULL,
                arguments TEXT NOT NULL,
                priority INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'pending',
                created_at INTEGER NOT NULL,
                started_at INTEGER,
                completed_at INTEGER,
                result TEXT,
                retries INTEGER NOT NULL DEFAULT 0,
                max_retries INTEGER NOT NULL DEFAULT 3,
                context TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_jobs_status ON jobs(status);
            CREATE INDEX IF NOT EXISTS idx_jobs_priority ON jobs(priority DESC);
            CREATE INDEX IF NOT EXISTS idx_jobs_created ON jobs(created_at);
        ")?;
        Ok(())
    }
}
