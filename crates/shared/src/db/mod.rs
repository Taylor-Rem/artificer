mod schema;

use std::sync::{Arc, Mutex, MutexGuard};
use std::cell::RefCell;
use rusqlite::Connection;
use anyhow::Result;
use serde_json::Value;
use once_cell::sync::OnceCell;

#[derive(Clone, Debug)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Default for Db {
    fn default() -> Self {
        let db_path = std::env::current_dir()
            .expect("Could not get current directory")
            .join("memory.db");

        if let Some(parent) = db_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let conn = Connection::open(&db_path).expect("Failed to open database");

        conn.busy_timeout(std::time::Duration::from_secs(5))
            .expect("Failed to set busy timeout");
        conn.execute("PRAGMA foreign_keys = ON", [])
            .expect("Failed to enable foreign keys");

        schema::create_tables(&conn).expect("Failed to create tables");

        Self {
            conn: Arc::new(Mutex::new(conn)),
        }
    }
}

// Core DB methods
impl Db {
    pub fn lock(&self) -> Result<MutexGuard<'_, Connection>> {
        self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))
    }

    pub fn query(&self, sql: &str, params: impl rusqlite::Params) -> Result<String> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(sql)?;
        let column_names: Vec<String> = stmt.column_names()
            .iter()
            .map(|s| s.to_string())
            .collect();

        let rows: Vec<Value> = stmt
            .query_map(params, |row| {
                let mut map = serde_json::Map::new();
                for (i, name) in column_names.iter().enumerate() {
                    let val: rusqlite::types::Value = row.get(i)?;
                    let json_val = match val {
                        rusqlite::types::Value::Null => Value::Null,
                        rusqlite::types::Value::Integer(n) => serde_json::json!(n),
                        rusqlite::types::Value::Real(f) => serde_json::json!(f),
                        rusqlite::types::Value::Text(s) => serde_json::json!(s),
                        rusqlite::types::Value::Blob(b) => {
                            serde_json::json!(format!("<blob:{} bytes>", b.len()))
                        }
                    };
                    map.insert(name.clone(), json_val);
                }
                Ok(Value::Object(map))
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(serde_json::json!(rows).to_string())
    }

    pub fn execute(&self, sql: &str, params: impl rusqlite::Params) -> Result<usize> {
        let conn = self.lock()?;
        Ok(conn.execute(sql, params)?)
    }

    pub fn query_row_optional<T, F>(
        &self,
        sql: &str,
        params: impl rusqlite::Params,
        f: F,
    ) -> Result<Option<T>>
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

    pub fn create_job(
        &self,
        device_id: i64,
        task_title: &str,
        arguments: &Value,
        priority: u32,
    ) -> Result<u64> {
        let conn = self.lock()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;

        conn.execute(
            "INSERT INTO background (device_id, method, arguments, priority, status, created_at)
             VALUES (?1, ?2, ?3, ?4, 'pending', ?5)",
            rusqlite::params![device_id, task_title, arguments.to_string(), priority, now],
        )?;

        Ok(conn.last_insert_rowid() as u64)
    }

    pub fn register_task(&self, id: i64, title: &str, description: &str) -> Result<()> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT OR IGNORE INTO tasks (id, title, description) VALUES (?1, ?2, ?3)",
            rusqlite::params![id, title, description],
        )?;
        Ok(())
    }
}

// Global instance management
static DB_INSTANCE: OnceCell<Arc<Db>> = OnceCell::new();

pub fn init() -> Arc<Db> {
    let db = Arc::new(Db::default());
    DB_INSTANCE.set(db.clone()).expect("DB already initialized");
    db
}

pub fn get() -> &'static Arc<Db> {
    DB_INSTANCE.get().expect("DB not initialized - call db::init() first")
}

// Device context management for tool scoping
thread_local! {
    static CURRENT_DEVICE_ID: RefCell<Option<i64>> = RefCell::new(None);
}

pub fn set_device_context(device_id: i64) -> Result<()> {
    CURRENT_DEVICE_ID.with(|id| {
        *id.borrow_mut() = Some(device_id);
    });

    get().execute(
        "INSERT OR REPLACE INTO runtime_context (key, value) VALUES ('current_device_id', ?)",
        rusqlite::params![device_id],
    )?;
    Ok(())
}

pub fn get_device_context() -> Option<i64> {
    CURRENT_DEVICE_ID.with(|id| *id.borrow())
}

// Helper for shared that need to convert JSON to rusqlite params
pub fn json_to_rusqlite(val: &Value) -> rusqlite::types::Value {
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