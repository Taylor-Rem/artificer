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

        -- Tasks (device-specific)
        -- One row per user request the Orchestrator works on.
        -- Created when the Orchestrator starts work, updated at checkpoints, finalized on completion.
        CREATE TABLE IF NOT EXISTS tasks (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            device_id INTEGER NOT NULL,
            conversation_id INTEGER NOT NULL,
            goal TEXT NOT NULL,
            title TEXT,
            summary TEXT,
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
                ON DELETE CASCADE ON UPDATE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_tasks_device ON tasks(device_id);
        CREATE INDEX IF NOT EXISTS idx_tasks_conversation ON tasks(conversation_id);
        CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);

        -- Keywords (global - shared across conversations and tasks)
        CREATE TABLE IF NOT EXISTS keywords (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            keyword TEXT NOT NULL UNIQUE
        );
        CREATE INDEX IF NOT EXISTS idx_keyword ON keywords(keyword);

        -- Conversation keywords
        CREATE TABLE IF NOT EXISTS conversation_keywords (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            conversation_id INTEGER NOT NULL,
            keyword_id INTEGER NOT NULL,
            FOREIGN KEY (conversation_id) REFERENCES conversations(id)
                ON DELETE CASCADE ON UPDATE CASCADE,
            FOREIGN KEY (keyword_id) REFERENCES keywords(id)
                ON DELETE CASCADE ON UPDATE CASCADE,
            UNIQUE(conversation_id, keyword_id)
        );
        CREATE INDEX IF NOT EXISTS idx_conv_keywords_conv ON conversation_keywords(conversation_id);
        CREATE INDEX IF NOT EXISTS idx_conv_keywords_keyword ON conversation_keywords(keyword_id);

        -- Task keywords (extracted after specialist completion)
        CREATE TABLE IF NOT EXISTS task_keywords (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            task_id INTEGER NOT NULL,
            keyword_id INTEGER NOT NULL,
            FOREIGN KEY (task_id) REFERENCES tasks(id)
                ON DELETE CASCADE ON UPDATE CASCADE,
            FOREIGN KEY (keyword_id) REFERENCES keywords(id)
                ON DELETE CASCADE ON UPDATE CASCADE,
            UNIQUE(task_id, keyword_id)
        );
        CREATE INDEX IF NOT EXISTS idx_task_keywords_task ON task_keywords(task_id);
        CREATE INDEX IF NOT EXISTS idx_task_keywords_keyword ON task_keywords(keyword_id);

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

        -- Long-term memory (device-specific)
        CREATE TABLE IF NOT EXISTS local_data (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            device_id INTEGER NOT NULL,
            task_id INTEGER,
            key TEXT NOT NULL,
            value TEXT NOT NULL,
            memory_type TEXT NOT NULL CHECK(memory_type IN ('fact', 'preference', 'context')),
            confidence REAL NOT NULL DEFAULT 1.0,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            last_accessed INTEGER,
            UNIQUE(device_id, key),
            FOREIGN KEY (device_id) REFERENCES devices(id)
                ON DELETE CASCADE ON UPDATE CASCADE,
            FOREIGN KEY (task_id) REFERENCES tasks(id)
                ON DELETE SET NULL ON UPDATE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_local_data_device ON local_data(device_id);
        CREATE INDEX IF NOT EXISTS idx_local_data_device_key ON local_data(key);
        CREATE INDEX IF NOT EXISTS idx_local_data_type ON local_data(memory_type);

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

        -- Runtime context for device scoping
        -- Keyed string store for lightweight session state
        CREATE TABLE IF NOT EXISTS runtime_context (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        -- ----------------------------------------------------------------
        -- Device-scoped views
        -- ----------------------------------------------------------------

        CREATE VIEW IF NOT EXISTS device_conversations AS
        SELECT c.* FROM conversations c
        WHERE c.device_id = (
            SELECT CAST(value AS INTEGER)
            FROM runtime_context WHERE key = 'current_device_id'
        );

        CREATE VIEW IF NOT EXISTS device_tasks AS
        SELECT t.* FROM tasks t
        WHERE t.device_id = (
            SELECT CAST(value AS INTEGER)
            FROM runtime_context WHERE key = 'current_device_id'
        );

        CREATE VIEW IF NOT EXISTS device_messages AS
        SELECT m.* FROM messages m
        JOIN conversations c ON m.conversation_id = c.id
        WHERE c.device_id = (
            SELECT CAST(value AS INTEGER)
            FROM runtime_context WHERE key = 'current_device_id'
        );

        CREATE VIEW IF NOT EXISTS device_local_data AS
        SELECT ltd.* FROM local_data ltd
        WHERE ltd.device_id = (
            SELECT CAST(value AS INTEGER)
            FROM runtime_context WHERE key = 'current_device_id'
        );

        CREATE VIEW IF NOT EXISTS device_conversation_keywords AS
        SELECT ck.* FROM conversation_keywords ck
        JOIN conversations c ON ck.conversation_id = c.id
        WHERE c.device_id = (
            SELECT CAST(value AS INTEGER)
            FROM runtime_context WHERE key = 'current_device_id'
        );

        CREATE VIEW IF NOT EXISTS device_task_keywords AS
        SELECT tk.* FROM task_keywords tk
        JOIN tasks t ON tk.task_id = t.id
        WHERE t.device_id = (
            SELECT CAST(value AS INTEGER)
            FROM runtime_context WHERE key = 'current_device_id'
        );

        -- Conversations with their keyword list
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
        WHERE c.device_id = (
            SELECT CAST(value AS INTEGER)
            FROM runtime_context WHERE key = 'current_device_id'
        )
        GROUP BY c.id, c.title, c.summary, c.created, c.last_accessed;

        -- Tasks with their keyword list — useful for Archivist queries
        CREATE VIEW IF NOT EXISTS device_tasks_with_keywords AS
        SELECT
            t.id,
            t.conversation_id,
            t.goal,
            t.title,
            t.summary,
            t.status,
            t.plan,
            t.created_at,
            t.completed_at,
            GROUP_CONCAT(k.keyword, ', ') as keywords
        FROM tasks t
        LEFT JOIN task_keywords tk ON t.id = tk.task_id
        LEFT JOIN keywords k ON tk.keyword_id = k.id
        WHERE t.device_id = (
            SELECT CAST(value AS INTEGER)
            FROM runtime_context WHERE key = 'current_device_id'
        )
        GROUP BY t.id, t.conversation_id, t.goal, t.title, t.summary,
                 t.status, t.plan, t.created_at, t.completed_at;
    ")?;
    Ok(())
}