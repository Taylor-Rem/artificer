use rusqlite::Connection;
use anyhow::Result;

pub fn create_tables(conn: &Connection) -> Result<()> {
    conn.execute_batch("
        -- Device registry
        CREATE TABLE IF NOT EXISTS devices (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            device_name TEXT NOT NULL UNIQUE,
            device_key TEXT NOT NULL UNIQUE,
            active INTEGER NOT NULL DEFAULT 1,
            created INTEGER NOT NULL,
            last_seen INTEGER NOT NULL,
            metadata TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_devices_name ON devices(device_name);
        CREATE INDEX IF NOT EXISTS idx_devices_key ON devices(device_key);

        -- Conversations (device-specific)
        -- A conversation is a session — one or more tasks within a continuous interaction
        CREATE TABLE IF NOT EXISTS conversations (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            device_id INTEGER NOT NULL,
            title TEXT,
            created INTEGER NOT NULL,
            last_accessed INTEGER NOT NULL,
            FOREIGN KEY (device_id) REFERENCES devices(id)
                ON DELETE CASCADE
                ON UPDATE CASCADE,
            UNIQUE(device_id, title)
        );
        CREATE INDEX IF NOT EXISTS idx_conversations_device ON conversations(device_id);
        CREATE INDEX IF NOT EXISTS idx_conversations_title ON conversations(device_id, title);

        -- Tasks (device-specific)
        -- One row per user request the Orchestrator works on.
        -- Created when the Orchestrator starts work, updated at checkpoints, finalized on completion.
        CREATE TABLE IF NOT EXISTS tasks (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            device_id INTEGER NOT NULL,
            conversation_id INTEGER NOT NULL,
            parent_task_id INTEGER,
            goal TEXT NOT NULL,
            title TEXT,
            plan TEXT,
            working_memory TEXT,
            status TEXT NOT NULL DEFAULT 'in_progress'
                CHECK(status IN ('in_progress', 'completed', 'failed', 'abandoned')),
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            completed_at INTEGER,

            FOREIGN KEY (device_id) REFERENCES devices(id)
                ON DELETE CASCADE ON UPDATE CASCADE,
            FOREIGN KEY (conversation_id) REFERENCES conversations(id)
                ON DELETE CASCADE ON UPDATE CASCADE,
            FOREIGN KEY (parent_task_id) REFERENCES tasks(id)
                ON DELETE CASCADE ON UPDATE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_tasks_device ON tasks(device_id);
        CREATE INDEX IF NOT EXISTS idx_tasks_conversation ON tasks(conversation_id);
        CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
        CREATE INDEX IF NOT EXISTS idx_tasks_parent ON tasks(parent_task_id);

        -- Messages (device-specific via conversation)
        CREATE TABLE IF NOT EXISTS messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            conversation_id INTEGER NOT NULL,
            task_id INTEGER,
            role TEXT NOT NULL,
            message TEXT,
            tool_calls TEXT,
            m_order INTEGER NOT NULL,
            created INTEGER NOT NULL,
            FOREIGN KEY (conversation_id) REFERENCES conversations(id)
                ON DELETE CASCADE ON UPDATE CASCADE,
            FOREIGN KEY (task_id) REFERENCES tasks(id)
                ON DELETE SET NULL ON UPDATE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_messages_conversation ON messages(conversation_id);
        CREATE INDEX IF NOT EXISTS idx_messages_task ON messages(task_id);

        -- Background jobs
        CREATE TABLE IF NOT EXISTS background (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            device_id INTEGER,
            -- What kind of job this is
            method TEXT NOT NULL,
            arguments TEXT NOT NULL,
            priority INTEGER NOT NULL DEFAULT 0,
            status TEXT NOT NULL DEFAULT 'pending'
                CHECK(status IN ('pending', 'running', 'completed', 'failed')),
            created_at INTEGER NOT NULL,
            started_at INTEGER,
            completed_at INTEGER,
            result TEXT,
            retries INTEGER NOT NULL DEFAULT 0,
            max_retries INTEGER NOT NULL DEFAULT 3,
            FOREIGN KEY (device_id) REFERENCES devices(id)
                ON DELETE SET NULL ON UPDATE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_jobs_status ON background(status);
        CREATE INDEX IF NOT EXISTS idx_jobs_device ON background(device_id);
        CREATE INDEX IF NOT EXISTS idx_jobs_priority ON background(priority DESC);
    ")?;
    Ok(())
}
