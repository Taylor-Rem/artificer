mod schema;

use std::sync::{Arc, Mutex, MutexGuard};
use rusqlite::Connection;
use anyhow::Result;
use serde_json::Value;
use once_cell::sync::OnceCell;

use crate::{Message, ToolCall};

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
        conn.execute_batch("
            PRAGMA foreign_keys = ON;
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
        ").expect("Failed to set pragmas");

        schema::create_tables(&conn).expect("Failed to create tables");

        Self {
            conn: Arc::new(Mutex::new(conn)),
        }
    }
}

// ============================================================================
// CORE DB PRIMITIVES
// ============================================================================

impl Db {
    pub fn lock(&self) -> Result<MutexGuard<'_, Connection>> {
        self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))
    }

    /// Run a SELECT and return results as a JSON string.
    /// Useful for passing query results to the LLM or tool responses.
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
}

// ============================================================================
// CONVERSATIONS
// ============================================================================

impl Db {
    /// Create a new conversation for a device. Returns the new conversation_id.
    pub fn create_conversation(&self, device_id: u64) -> Result<u64> {
        let conn = self.lock()?;
        let now = now();

        let device_exists: bool = conn.query_row(
            "SELECT 1 FROM devices WHERE id = ?1",
            rusqlite::params![device_id],
            |_| Ok(true),
        ).unwrap_or(false);

        if !device_exists {
            return Err(anyhow::anyhow!(
                "Device {} does not exist. Register the device before creating conversations.",
                device_id
            ));
        }

        conn.execute(
            "INSERT INTO conversations (device_id, created, last_accessed)
             VALUES (?1, ?2, ?3)",
            rusqlite::params![device_id, now, now],
        )?;

        Ok(conn.last_insert_rowid() as u64)
    }

    /// Touch last_accessed on a conversation.
    pub fn touch_conversation(&self, conversation_id: u64) -> Result<()> {
        self.execute(
            "UPDATE conversations SET last_accessed = ?1 WHERE id = ?2",
            rusqlite::params![now(), conversation_id as i64],
        )?;
        Ok(())
    }

    pub fn get_conversation_title(&self, conversation_id: u64) -> Result<Option<String>> {
        self.query_row_optional(
            "SELECT title FROM conversations WHERE id = ?1",
            rusqlite::params![conversation_id as i64],
            |row| row.get(0),
        )
    }
}

// ============================================================================
// MESSAGES
// ============================================================================

impl Db {
    /// Add a message to a conversation. Increments message_count in place.
    pub fn add_message(
        &self,
        conversation_id: u64,
        task_id: Option<i64>,
        role: &str,
        content: Option<&str>,
        tool_calls: Option<&Vec<ToolCall>>,
        message_count: &mut u32,
    ) -> Result<()> {
        let tool_calls_json = tool_calls
            .map(|tc| serde_json::to_string(tc))
            .transpose()?;

        let conn = self.lock()?;
        let now = now();

        conn.execute(
            "INSERT INTO messages
             (conversation_id, task_id, role, message, tool_calls, m_order, created)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                conversation_id as i64,
                task_id,
                role,
                content,
                tool_calls_json,
                *message_count as i64,
                now,
            ],
        )?;
        *message_count += 1;

        conn.execute(
            "UPDATE conversations SET last_accessed = ?1 WHERE id = ?2",
            rusqlite::params![now, conversation_id as i64],
        )?;

        Ok(())
    }

    /// Load all messages for a conversation in order.
    pub fn get_messages(&self, conversation_id: u64) -> Result<Vec<Message>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT role, message, tool_calls FROM messages
             WHERE conversation_id = ?1
             ORDER BY m_order",
        )?;

        let messages = stmt.query_map(
            rusqlite::params![conversation_id as i64],
            |row| {
                let role: String = row.get(0)?;
                let message: Option<String> = row.get(1)?;
                let tool_calls_json: Option<String> = row.get(2)?;
                Ok((role, message, tool_calls_json))
            },
        )?
            .filter_map(|r| r.ok())
            .map(|(role, message, tool_calls_json)| {
                let tool_calls = tool_calls_json
                    .and_then(|j| serde_json::from_str(&j).ok());
                Message { role, content: message, tool_calls }
            })
            .collect();

        Ok(messages)
    }

    /// Get the current message count for a conversation (for ordered inserts).
    pub fn get_message_count(&self, conversation_id: u64) -> Result<u32> {
        let conn = self.lock()?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE conversation_id = ?1",
            rusqlite::params![conversation_id as i64],
            |row| row.get(0),
        )?;
        Ok(count as u32)
    }
}

// ============================================================================
// TASKS
// ============================================================================

impl Db {
    /// Create a new task record. Returns the task_id.
    pub fn create_task(
        &self,
        device_id: u64,
        conversation_id: u64,
        parent_task_id: Option<u64>,
        goal: &str,
    ) -> Result<u64> {
        let now = now();
        let conn = self.lock()?;

        conn.execute(
            "INSERT INTO tasks
             (device_id, conversation_id, parent_task_id, goal, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 'in_progress', ?5, ?6)",
            rusqlite::params![
                device_id,
                conversation_id as i64,
                parent_task_id.map(|id| id as i64),
                goal,
                now,
                now
            ],
        )?;

        Ok(conn.last_insert_rowid() as u64)
    }

    /// Checkpoint: persist current plan and working memory.
    pub fn checkpoint_task(
        &self,
        task_id: i64,
        plan: Option<&str>,
        working_memory: Option<&str>,
    ) -> Result<()> {
        self.execute(
            "UPDATE tasks SET plan = ?1, working_memory = ?2, updated_at = ?3
             WHERE id = ?4",
            rusqlite::params![plan, working_memory, now(), task_id],
        )?;
        Ok(())
    }

    /// Mark a task as completed.
    pub fn complete_task(&self, task_id: i64) -> Result<()> {
        let now = now();
        self.execute(
            "UPDATE tasks SET status = 'completed', completed_at = ?1, updated_at = ?2
             WHERE id = ?3",
            rusqlite::params![now, now, task_id],
        )?;
        Ok(())
    }

    /// Mark a task as failed.
    pub fn fail_task(&self, task_id: i64) -> Result<()> {
        let now = now();
        self.execute(
            "UPDATE tasks SET status = 'failed', completed_at = ?1, updated_at = ?2
             WHERE id = ?3",
            rusqlite::params![now, now, task_id],
        )?;
        Ok(())
    }

    /// Get goal and plan for a task by ID. Used for parent task queries.
    pub fn get_task_info(&self, task_id: u64) -> Result<Option<(String, Option<String>)>> {
        self.query_row_optional(
            "SELECT goal, plan FROM tasks WHERE id = ?1",
            rusqlite::params![task_id as i64],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
    }
}

// ============================================================================
// TITLES
// ============================================================================

impl Db {
    pub fn conversation_title_exists(&self, device_id: i64, title: &str) -> bool {
        if let Ok(conn) = self.lock() {
            conn.query_row(
                "SELECT 1 FROM conversations WHERE device_id = ?1 AND title = ?2 LIMIT 1",
                rusqlite::params![device_id, title],
                |_| Ok(true),
            ).unwrap_or(false)
        } else {
            false
        }
    }

    pub fn find_available_conversation_title(&self, device_id: i64, base: &str) -> String {
        let mut counter = 1u32;
        loop {
            let candidate = format!("{}_{}", base, counter);
            if !self.conversation_title_exists(device_id, &candidate) {
                return candidate;
            }
            counter += 1;
            if counter > 1000 {
                return format!("{}_{}", base, &uuid::Uuid::new_v4().to_string()[..8]);
            }
        }
    }

    pub fn set_conversation_title(
        &self,
        conversation_id: u64,
        device_id: i64,
        raw_title: &str,
    ) -> Result<String> {
        let sanitized = sanitize_title(raw_title);
        if sanitized.is_empty() {
            return Err(anyhow::anyhow!("Title is empty after sanitization"));
        }

        let final_title = if self.conversation_title_exists(device_id, &sanitized) {
            self.find_available_conversation_title(device_id, &sanitized)
        } else {
            sanitized
        };

        self.execute(
            "UPDATE conversations SET title = ?1 WHERE id = ?2",
            rusqlite::params![final_title, conversation_id as i64],
        )?;

        Ok(final_title)
    }

    pub fn set_task_title(&self, task_id: i64, title: &str) -> Result<()> {
        let sanitized = sanitize_title(title);
        if sanitized.is_empty() {
            return Err(anyhow::anyhow!("Title is empty after sanitization"));
        }
        self.execute(
            "UPDATE tasks SET title = ?1 WHERE id = ?2",
            rusqlite::params![sanitized, task_id],
        )?;
        Ok(())
    }
}

// ============================================================================
// BACKGROUND JOBS
// ============================================================================

impl Db {
    /// Queue a title generation job for a conversation.
    pub fn queue_title_generation(
        &self,
        device_id: i64,
        conversation_id: u64,
        first_message: &str,
    ) -> Result<u64> {
        self.create_job(
            device_id,
            "title_generation",
            &serde_json::json!({
                "conversation_id": conversation_id,
                "user_message": first_message,
            }),
            1,
        )
    }

    /// Clean up old completed/failed background jobs older than 7 days.
    pub fn cleanup_old_background_jobs(&self) -> Result<usize> {
        let seven_days_ago = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64 - (7 * 24 * 60 * 60);

        let conn = self.lock()?;
        let count = conn.execute(
            "DELETE FROM background
             WHERE status IN ('completed', 'failed')
             AND created_at < ?1",
            rusqlite::params![seven_days_ago],
        )?;

        Ok(count)
    }

    pub fn create_job(
        &self,
        device_id: i64,
        method: &str,
        arguments: &Value,
        priority: u32,
    ) -> Result<u64> {
        let conn = self.lock()?;
        let now = now();

        conn.execute(
            "INSERT INTO background
             (device_id, method, arguments, priority, status, created_at)
             VALUES (?1, ?2, ?3, ?4, 'pending', ?5)",
            rusqlite::params![
                device_id,
                method,
                arguments.to_string(),
                priority,
                now
            ],
        )?;

        Ok(conn.last_insert_rowid() as u64)
    }
}

// ============================================================================
// EXECUTION TRACES
// ============================================================================

impl Db {
    /// Log a single iteration of an agent's execution loop.
    #[allow(clippy::too_many_arguments)]
    pub fn log_execution_trace(
        &self,
        task_id: u64,
        agent_name: &str,
        iteration: u32,
        system_prompt_preview: Option<&str>,
        input_context: &str,
        reasoning: Option<&str>,
        tool_calls: Option<&str>,
        tool_results: Option<&str>,
        classification: &str,
        llm_duration_ms: Option<u64>,
    ) -> Result<()> {
        let now = now();
        self.execute(
            "INSERT INTO execution_traces
             (task_id, agent_name, iteration, system_prompt_preview, input_context,
              reasoning, tool_calls, tool_results, classification, created_at, llm_duration_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                task_id as i64,
                agent_name,
                iteration as i64,
                system_prompt_preview,
                input_context,
                reasoning,
                tool_calls,
                tool_results,
                classification,
                now,
                llm_duration_ms.map(|ms| ms as i64),
            ],
        )?;
        Ok(())
    }

    /// Get all traces for a task, ordered by iteration.
    pub fn get_execution_traces(&self, task_id: u64) -> Result<String> {
        self.query(
            "SELECT iteration, agent_name, classification, reasoning, tool_calls, tool_results,
                    llm_duration_ms, created_at
             FROM execution_traces
             WHERE task_id = ?1
             ORDER BY iteration",
            rusqlite::params![task_id as i64],
        )
    }

    /// Get the full detail for a specific iteration of a task trace.
    pub fn get_execution_trace_detail(&self, task_id: u64, iteration: u32) -> Result<String> {
        self.query(
            "SELECT *
             FROM execution_traces
             WHERE task_id = ?1 AND iteration = ?2",
            rusqlite::params![task_id as i64, iteration as i64],
        )
    }
}

// ============================================================================
// GLOBAL INSTANCE
// ============================================================================

static DB_INSTANCE: OnceCell<Arc<Db>> = OnceCell::new();

pub fn init() -> Arc<Db> {
    let db = Arc::new(Db::default());
    DB_INSTANCE.set(db.clone()).expect("DB already initialized");
    db
}

pub fn get() -> &'static Arc<Db> {
    DB_INSTANCE.get().expect("DB not initialized — call db::init() first")
}

// ============================================================================
// HELPERS
// ============================================================================

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

pub fn sanitize_title(title: &str) -> String {
    title.chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' => c,
            ' ' | '-' | '.' | '/' | '\\' => '_',
            _ => '_',
        })
        .collect::<String>()
        .split('_')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}

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
