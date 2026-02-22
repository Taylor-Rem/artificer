use rusqlite::Connection;
use anyhow::Result;

pub fn create_tables(conn: &Connection) -> Result<()> {
    conn.execute_batch("
        -- Device registry
        CREATE TABLE IF NOT EXISTS devices (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            device_name TEXT NOT NULL UNIQUE,
            device_key TEXT NOT NULL UNIQUE,
            created INTEGER NOT NULL,
            last_seen INTEGER NOT NULL,
            metadata TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_devices_name ON devices(device_name);
        CREATE INDEX IF NOT EXISTS idx_devices_key ON devices(device_key);

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

        -- Keywords (global)
        CREATE TABLE IF NOT EXISTS keywords (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            keyword TEXT NOT NULL UNIQUE
        );
        CREATE INDEX IF NOT EXISTS idx_keyword ON keywords(keyword);

        -- Conversation keywords (device-specific via conversation)
        CREATE TABLE IF NOT EXISTS conversation_keywords (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            conversation_id INTEGER NOT NULL,
            keyword_id INTEGER NOT NULL,
            FOREIGN KEY (conversation_id) REFERENCES conversations(id)
                ON DELETE CASCADE
                ON UPDATE CASCADE,
            FOREIGN KEY (keyword_id) REFERENCES keywords(id)
                ON DELETE CASCADE
                ON UPDATE CASCADE,
            UNIQUE(conversation_id, keyword_id)
        );
        CREATE INDEX IF NOT EXISTS idx_conversation_keywords_conv ON conversation_keywords(conversation_id);
        CREATE INDEX IF NOT EXISTS idx_conversation_keywords_keyword ON conversation_keywords(keyword_id);

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
        CREATE TABLE IF NOT EXISTS local_data (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            device_id INTEGER NOT NULL,
            task_id INTEGER NOT NULL,
            conversation_id INTEGER,
            key TEXT NOT NULL,
            value TEXT NOT NULL,
            memory_type TEXT NOT NULL CHECK(memory_type IN ('fact', 'preference', 'context')),
            confidence REAL NOT NULL DEFAULT 1.0,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            last_accessed INTEGER,
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
        );
        CREATE INDEX IF NOT EXISTS idx_local_data_device ON local_data(device_id);
        CREATE INDEX IF NOT EXISTS idx_local_data_task ON local_data(device_id, task_id);
        CREATE INDEX IF NOT EXISTS idx_local_data_type ON local_data(memory_type);

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

        -- Runtime context for device scoping
        CREATE TABLE IF NOT EXISTS runtime_context (
            key TEXT PRIMARY KEY,
            value INTEGER NOT NULL
        );

        -- Device-scoped views for secure access
        CREATE VIEW IF NOT EXISTS device_conversations AS
        SELECT c.* FROM conversations c
        WHERE c.device_id = (SELECT value FROM runtime_context WHERE key = 'current_device_id');

        CREATE VIEW IF NOT EXISTS device_messages AS
        SELECT m.* FROM messages m
        JOIN conversations c ON m.conversation_id = c.id
        WHERE c.device_id = (SELECT value FROM runtime_context WHERE key = 'current_device_id');

        CREATE VIEW IF NOT EXISTS device_task_history AS
        SELECT th.* FROM task_history th
        WHERE th.device_id = (SELECT value FROM runtime_context WHERE key = 'current_device_id');

        CREATE VIEW IF NOT EXISTS device_local_data AS
        SELECT ltd.* FROM local_data ltd
        WHERE ltd.device_id = (SELECT value FROM runtime_context WHERE key = 'current_device_id');

        CREATE VIEW IF NOT EXISTS device_conversation_keywords AS
        SELECT ck.* FROM conversation_keywords ck
        JOIN conversations c ON ck.conversation_id = c.id
        WHERE c.device_id = (SELECT value FROM runtime_context WHERE key = 'current_device_id');

        -- Convenient joined view for conversations with keywords
        CREATE VIEW IF NOT EXISTS device_conversations_with_keywords AS
        SELECT
            c.id,
            c.title,
            c.summary,
            c.created,
            c.last_accessed,
            GROUP_CONCAT(k.keyword, ', ') as keywords
        FROM conversations c
        LEFT JOIN conversation_keywords ck ON c.id = ck.conversation_id
        LEFT JOIN keywords k ON ck.keyword_id = k.id
        WHERE c.device_id = (SELECT value FROM runtime_context WHERE key = 'current_device_id')
        GROUP BY c.id, c.title, c.summary, c.created, c.last_accessed;
    ")?;
    Ok(())
}