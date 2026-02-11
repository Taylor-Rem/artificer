use crate::engine::db::Db;

pub struct Title {
    db: Db,
}

impl Default for Title {
    fn default() -> Self {
        Self { db: Db::default() }
    }
}

impl Title {
    pub fn sanitize_title(&self, title: &str) -> String {
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

    pub fn title_exists(&self, title: &str) -> bool {
        if let Ok(conn) = self.db.lock() {
            let exists: bool = conn
                .query_row(
                    "SELECT 1 FROM conversation WHERE title = ?1 LIMIT 1",
                    rusqlite::params![title],
                    |_row| Ok(true),
                )
                .unwrap_or(false);
            return exists;
        }
        false
    }

    pub fn find_available_title(&self, base: &str) -> String {
        let mut counter = 1;
        loop {
            let candidate = format!("{}_{}", base, counter);
            if !self.title_exists(&candidate) {
                return candidate;
            }
            counter += 1;

            if counter > 1000 {
                return format!("{}_{}", base, uuid::Uuid::new_v4().to_string());
            }
        }
    }
}
