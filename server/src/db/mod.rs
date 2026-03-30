use crate::client_manager::ClientMetadata;
use rusqlite::{params, Connection};
use std::path::Path;

pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn open(path: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        if let Some(parent) = Path::new(path).parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        let db = Self { conn };
        db.init()?;
        Ok(db)
    }

    fn init(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS sessions (
                install_id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                build_uuid TEXT NOT NULL,
                hostname TEXT NOT NULL,
                username TEXT NOT NULL,
                pid INTEGER NOT NULL,
                ip TEXT NOT NULL,
                os TEXT NOT NULL,
                arch TEXT NOT NULL,
                status TEXT NOT NULL,
                connected_at INTEGER NOT NULL,
                last_seen INTEGER NOT NULL,
                disconnected_at INTEGER
            );

            CREATE INDEX IF NOT EXISTS idx_sessions_status ON sessions(status);

            CREATE TABLE IF NOT EXISTS command_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                install_id TEXT NOT NULL,
                tool_name TEXT NOT NULL,
                ok INTEGER NOT NULL,
                data_json TEXT NOT NULL,
                error TEXT NOT NULL,
                duration_ms INTEGER NOT NULL,
                created_at INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_command_history_install_id ON command_history(install_id);

            CREATE TABLE IF NOT EXISTS chat_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                install_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_chat_history_install_id ON chat_history(install_id);

            CREATE TABLE IF NOT EXISTS conversations (
                conversation_id TEXT PRIMARY KEY,
                install_id TEXT NOT NULL,
                title TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_conversations_install_id ON conversations(install_id);
            "#,
        )?;
        Ok(())
    }

    pub fn upsert_session_online(
        &self,
        install_id: &str,
        session_id: &str,
        meta: &ClientMetadata,
        now_ts: u64,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.conn.execute(
            r#"
            INSERT INTO sessions (
                install_id, session_id, build_uuid,
                hostname, username, pid, ip, os, arch,
                status, connected_at, last_seen, disconnected_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'online', ?10, ?11, NULL)
            ON CONFLICT(install_id) DO UPDATE SET
                session_id=excluded.session_id,
                build_uuid=excluded.build_uuid,
                hostname=excluded.hostname,
                username=excluded.username,
                pid=excluded.pid,
                ip=excluded.ip,
                os=excluded.os,
                arch=excluded.arch,
                status='online',
                last_seen=excluded.last_seen,
                disconnected_at=NULL
            "#,
            params![
                install_id,
                session_id,
                meta.build_uuid,
                meta.hostname,
                meta.username,
                meta.pid,
                meta.ip,
                meta.os,
                meta.arch,
                meta.registered_at,
                now_ts,
            ],
        )?;
        Ok(())
    }

    pub fn mark_offline(
        &self,
        session_id: &str,
        now_ts: u64,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.conn.execute(
            "UPDATE sessions SET status='offline', disconnected_at=?2, last_seen=?2 WHERE session_id=?1",
            params![session_id, now_ts],
        )?;
        Ok(())
    }

    pub fn insert_command_history(
        &self,
        session_id: &str,
        install_id: &str,
        tool_name: &str,
        ok: bool,
        data_json: &str,
        error: &str,
        duration_ms: u64,
        now_ts: u64,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.conn.execute(
            "INSERT INTO command_history(session_id, install_id, tool_name, ok, data_json, error, duration_ms, created_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![session_id, install_id, tool_name, if ok {1} else {0}, data_json, error, duration_ms as i64, now_ts],
        )?;
        Ok(())
    }

    pub fn insert_chat(
        &self,
        install_id: &str,
        role: &str,
        content: &str,
        now_ts: u64,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.conn.execute(
            "INSERT INTO chat_history(install_id, role, content, created_at) VALUES(?1, ?2, ?3, ?4)",
            params![install_id, role, content, now_ts],
        )?;
        Ok(())
    }

    pub fn read_recent_chat(
        &self,
        install_id: &str,
        limit: usize,
    ) -> Result<Vec<(String, String)>, Box<dyn std::error::Error + Send + Sync>> {
        let mut stmt = self.conn.prepare(
            "SELECT role, content FROM chat_history WHERE install_id=?1 ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![install_id, limit as i64], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        out.reverse();
        Ok(out)
    }

    pub fn read_chat_history(
        &self,
        install_id: &str,
        limit: usize,
    ) -> Result<Vec<(String, String, u64)>, Box<dyn std::error::Error + Send + Sync>> {
        let mut stmt = self.conn.prepare(
            "SELECT role, content, created_at FROM chat_history WHERE install_id=?1 ORDER BY id ASC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![install_id, limit as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)? as u64,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn clear_install_history(
        &self,
        install_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.conn.execute(
            "DELETE FROM chat_history WHERE install_id=?1",
            params![install_id],
        )?;
        self.conn.execute(
            "DELETE FROM command_history WHERE install_id=?1",
            params![install_id],
        )?;
        Ok(())
    }

    pub fn insert_conversation(
        &self,
        conversation_id: &str,
        install_id: &str,
        title: &str,
        now_ts: u64,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.conn.execute(
            "INSERT OR REPLACE INTO conversations(conversation_id, install_id, title, created_at) VALUES(?1, ?2, ?3, ?4)",
            params![conversation_id, install_id, title, now_ts],
        )?;
        Ok(())
    }

    pub fn get_conversation_install_id(
        &self,
        conversation_id: &str,
    ) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
        let mut stmt = self
            .conn
            .prepare("SELECT install_id FROM conversations WHERE conversation_id=?1 LIMIT 1")?;
        let mut rows = stmt.query(params![conversation_id])?;
        if let Some(row) = rows.next()? {
            return Ok(Some(row.get::<_, String>(0)?));
        }
        Ok(None)
    }
}
