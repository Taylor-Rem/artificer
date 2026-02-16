use std::{sync::{Arc, Mutex, MutexGuard}, time::{SystemTime, UNIX_EPOCH}};
use anyhow::Result;
use rusqlite::Connection;
use serde_json::{json, Value};
use crate::task::Task;
use artificer_tools::db::DatabaseBackend;

#[derive(Clone)]
pub struct Db {
    db: Arc<Mutex<Connection>>,
}

impl Default for Db {
    fn default() -> Self {
        let db_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("memory")
            .join("memory.db");

        if let Some(parent) = db_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let conn = Connection::open(&db_path).expect("Failed to open database");

        // Set busy timeout - wait up to 5 seconds if database is locked
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .expect("Failed to set busy timeout");

        // Enable foreign keys
        conn.execute("PRAGMA foreign_keys = ON", [])
            .expect("Failed to enable foreign keys");

        Self::create_tables(&conn).expect("Failed to create tables");
        Self::populate_tables(&conn).expect("Failed to populate tables");

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

    pub fn query_row_optional<T, F>(&self, sql: &str, params: impl rusqlite::Params, f: F) -> Result<Option<T>>
    where
        F: FnOnce(&rusqlite::Row) -> rusqlite::Result<T>,
    {
        let conn = self.lock()?;
        match conn.query_row(sql, params, f) {
            Ok(val) => Ok(Some(val)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn lock(&self) -> Result<MutexGuard<'_, Connection>> {
        self.db.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))
    }

    pub fn create_job(&self, device_id: i64, task: Task, arguments: &serde_json::Value, priority: u32) -> Result<u64> {
        let conn = self.lock()?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_secs() as i64;

        conn.execute(
            "INSERT INTO background (device_id, method, arguments, priority, status, created_at)
             VALUES (?1, ?2, ?3, ?4, 'pending', ?5)",
            rusqlite::params![
                device_id,
                task.title(),
                arguments.to_string(),
                priority,
                now,
            ],
        )?;

        Ok(conn.last_insert_rowid() as u64)
    }

    fn create_tables(conn: &Connection) -> Result<()> {
        conn.execute_batch("
            -- Device registry
            CREATE TABLE IF NOT EXISTS devices (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                device_name TEXT NOT NULL UNIQUE,
                created INTEGER NOT NULL,
                last_seen INTEGER NOT NULL,
                metadata TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_devices_name ON devices(device_name);

            -- Task definitions (global - same across all devices)
            CREATE TABLE IF NOT EXISTS tasks (
                id INTEGER PRIMARY KEY,
                title TEXT NOT NULL UNIQUE,
                description TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_tasks_title ON tasks(title);

            -- Conversations (device-specific)
            CREATE TABLE IF NOT EXISTS conversations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                device_id INTEGER NOT NULL,
                title TEXT,
                summary TEXT,
                created INTEGER NOT NULL,
                last_accessed INTEGER NOT NULL,
                FOREIGN KEY (device_id) REFERENCES devices(id)
                    ON DELETE CASCADE
                    ON UPDATE CASCADE,
                UNIQUE(device_id, title)
            );
            CREATE INDEX IF NOT EXISTS idx_conversations_device ON conversations(device_id);
            CREATE INDEX IF NOT EXISTS idx_conversations_title ON conversations(device_id, title);

            -- Task execution history (device-specific)
            CREATE TABLE IF NOT EXISTS task_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                device_id INTEGER NOT NULL,
                task_id INTEGER NOT NULL,
                conversation_id INTEGER,
                location TEXT NOT NULL,
                created INTEGER NOT NULL,
                completed INTEGER,
                status TEXT NOT NULL DEFAULT 'running',
                FOREIGN KEY (device_id) REFERENCES devices(id)
                    ON DELETE CASCADE
                    ON UPDATE CASCADE,
                FOREIGN KEY (task_id) REFERENCES tasks(id)
                    ON DELETE CASCADE
                    ON UPDATE CASCADE,
                FOREIGN KEY (conversation_id) REFERENCES conversations(id)
                    ON DELETE SET NULL
                    ON UPDATE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_task_history_device ON task_history(device_id);
            CREATE INDEX IF NOT EXISTS idx_task_history_conversation ON task_history(conversation_id);

            -- Messages (device-specific via conversation)
            CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                conversation_id INTEGER NOT NULL,
                role TEXT NOT NULL,
                message TEXT NOT NULL,
                m_order INTEGER NOT NULL,
                created INTEGER NOT NULL,
                FOREIGN KEY (conversation_id) REFERENCES conversations(id)
                    ON DELETE CASCADE
                    ON UPDATE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_messages_conversation ON messages(conversation_id);

            -- Local task data (device-specific)
            CREATE TABLE IF NOT EXISTS local_task_data (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                device_id INTEGER NOT NULL,
                task_id INTEGER NOT NULL,
                conversation_id INTEGER,
                task_history_id INTEGER,
                key TEXT NOT NULL,
                value TEXT NOT NULL,
                memory_type TEXT NOT NULL CHECK(memory_type IN ('fact', 'preference', 'context')),
                confidence REAL NOT NULL DEFAULT 1.0,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                last_accessed INTEGER,  -- Track when this was used
                UNIQUE(device_id, task_id, key),
                FOREIGN KEY (device_id) REFERENCES devices(id)
                    ON DELETE CASCADE
                    ON UPDATE CASCADE,
                FOREIGN KEY (task_id) REFERENCES tasks(id)
                    ON DELETE CASCADE
                    ON UPDATE CASCADE,
                FOREIGN KEY (conversation_id) REFERENCES conversations(id)
                    ON DELETE CASCADE
                    ON UPDATE CASCADE,
                FOREIGN KEY (task_history_id) REFERENCES task_history(id)
                    ON DELETE CASCADE
                    ON UPDATE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_local_task_data_device ON local_task_data(device_id);
            CREATE INDEX IF NOT EXISTS idx_local_task_data_task ON local_task_data(device_id, task_id);
            CREATE INDEX IF NOT EXISTS idx_local_task_data_type ON local_task_data(memory_type);

            -- Background jobs (track which device queued)
            CREATE TABLE IF NOT EXISTS background (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                device_id INTEGER,
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
                context TEXT,
                FOREIGN KEY (device_id) REFERENCES devices(id)
                    ON DELETE SET NULL
                    ON UPDATE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_jobs_status ON background(status);
            CREATE INDEX IF NOT EXISTS idx_jobs_device ON background(device_id);
            CREATE INDEX IF NOT EXISTS idx_jobs_priority ON background(priority DESC);
        ")?;
        Ok(())
    }

    fn populate_tables(conn: &Connection) -> Result<()> {
        for task in Task::all() {
            conn.execute(
                "INSERT OR IGNORE INTO tasks (id, title, description) VALUES (?1, ?2, ?3)",
                rusqlite::params![task.task_id(), task.title(), task.description()],
            )?;
        }
        Ok(())
    }

    fn json_to_rusqlite(val: &Value) -> rusqlite::types::Value {
        match val {
            Value::Null => rusqlite::types::Value::Null,
            Value::Bool(b) => rusqlite::types::Value::Integer(*b as i64),
            Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    rusqlite::types::Value::Integer(i)
                } else {
                    rusqlite::types::Value::Real(n.as_f64().unwrap_or(0.0))
                }
            }
            Value::String(s) => rusqlite::types::Value::Text(s.clone()),
            other => rusqlite::types::Value::Text(other.to_string()),
        }
    }
}

impl DatabaseBackend for Db {
    fn query(&self, sql: &str, params: Vec<Value>) -> Result<String> {
        let rusqlite_params: Vec<rusqlite::types::Value> = params.iter()
            .map(Db::json_to_rusqlite)
            .collect();
        self.query(sql, rusqlite::params_from_iter(rusqlite_params))
    }

    fn execute(&self, sql: &str, params: Vec<Value>) -> Result<usize> {
        let rusqlite_params: Vec<rusqlite::types::Value> = params.iter()
            .map(Db::json_to_rusqlite)
            .collect();
        self.execute(sql, rusqlite::params_from_iter(rusqlite_params))
    }
}