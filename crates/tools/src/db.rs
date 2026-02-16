// crates/tools/src/db.rs
// Database trait - engine implements this

use anyhow::Result;
use serde_json::Value;
use std::sync::{Arc, Mutex};

/// Trait that the engine's database must implement
pub trait DatabaseBackend: Send + Sync {
    fn query(&self, sql: &str, params: Vec<Value>) -> Result<String>;
    fn execute(&self, sql: &str, params: Vec<Value>) -> Result<usize>;
}

/// Global database instance - set by engine at startup
static DB_INSTANCE: Mutex<Option<Arc<dyn DatabaseBackend>>> = Mutex::new(None);

/// Engine calls this at startup to inject the DB
pub fn set_database(db: Arc<dyn DatabaseBackend>) {
    let mut instance = DB_INSTANCE.lock().unwrap();
    *instance = Some(db);
}

/// Run a query and return JSON string results
pub fn query(sql: &str, params: Vec<Value>) -> Result<String> {
    let instance = DB_INSTANCE.lock().unwrap();
    let db = instance.as_ref()
        .ok_or_else(|| anyhow::anyhow!("Database not initialized"))?;
    db.query(sql, params)
}

/// Run an execute statement and return rows affected
pub fn execute(sql: &str, params: Vec<Value>) -> Result<usize> {
    let instance = DB_INSTANCE.lock().unwrap();
    let db = instance.as_ref()
        .ok_or_else(|| anyhow::anyhow!("Database not initialized"))?;
    db.execute(sql, params)
}

/// Tools call this to execute queries (takes Value with {query, params} fields)
pub fn execute_query(args: &Value) -> Result<String> {
    let sql = args["query"].as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing 'query' parameter"))?;

    let params = args["params"].as_array()
        .map(|arr| arr.clone())
        .unwrap_or_default();

    query(sql, params)
}

pub fn execute_command(args: &Value) -> Result<usize> {
    let sql = args["query"].as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing 'query' parameter"))?;

    let params = args["params"].as_array()
        .map(|arr| arr.clone())
        .unwrap_or_default();

    execute(sql, params)
}
