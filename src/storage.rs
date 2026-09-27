use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, params};

use crate::model::{ChatMessage, ChatParams, ChatSession, SessionTools};

pub type StorageResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

pub struct Database {
    connection: Connection,
    saved_sessions: HashMap<String, String>,
    saved_messages: HashMap<String, String>,
    saved_active_id: String,
}

impl Database {
    pub fn open(path: &Path) -> StorageResult<Self> {
        let connection = Connection::open(path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")?;
        migrate(&connection)?;
        Ok(Self {
            connection,
            saved_sessions: HashMap::new(),
            saved_messages: HashMap::new(),
            saved_active_id: String::new(),
        })
    }

    pub fn load(&mut self) -> StorageResult<(String, Vec<ChatSession>)> {
        let active_id = self
            .connection
            .query_row(
                "SELECT value FROM metadata WHERE key = 'active_session_id'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap_or_default();
        let mut sessions = Vec::new();
        let mut stmt = self.connection.prepare(
            "SELECT id, title, folder, model, provider_id, created_at, updated_at,
                    pinned, favorite, title_auto, params, tools
             FROM sessions ORDER BY position, rowid",
        )?;
        let rows = stmt.query_map([], |row| {
            let params_json: String = row.get(10)?;
            let tools_json: String = row.get(11)?;
            Ok((
                ChatSession {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    folder: row.get(2)?,
                    model: row.get(3)?,
                    provider_id: row.get(4)?,
                    created_at: row.get(5)?,
                    updated_at: row.get(6)?,
                    messages: Vec::new(),
                    pinned: row.get::<_, i64>(7)? != 0,
                    favorite: row.get::<_, i64>(8)? != 0,
                    title_auto: row.get::<_, i64>(9)? != 0,
                    params: None,
                    tools: None,
                },
                params_json,
                tools_json,
            ))
        })?;
        for row in rows {
            let (mut session, params_json, tools_json) = row?;
            if !params_json.is_empty() {
                session.params = Some(serde_json::from_str::<ChatParams>(&params_json)?);
            }
            // 空串 = 没设过 = chat 模式（见 `SessionTools`），不写进数据免得每个会话都多一段
            if !tools_json.is_empty() {
                session.tools = Some(serde_json::from_str::<SessionTools>(&tools_json)?);
            }
            sessions.push(session);
        }
        drop(stmt);

        let mut messages_stmt = self
            .connection
            .prepare("SELECT payload FROM messages WHERE session_id = ?1 ORDER BY position, rowid")?;
        for session in &mut sessions {
            let rows = messages_stmt.query_map([&session.id], |row| row.get::<_, String>(0))?;
            for row in rows {
                session.messages.push(serde_json::from_str::<ChatMessage>(&row?)?);
            }
        }
        drop(messages_stmt);

        self.saved_sessions.clear();
        self.saved_messages.clear();
        for (position, session) in sessions.iter().enumerate() {
            self.saved_sessions
                .insert(session.id.clone(), session_signature(session, position)?);
            for (message_position, message) in session.messages.iter().enumerate() {
                self.saved_messages.insert(
                    message.id.clone(),
                    message_signature(&session.id, message, message_position)?,
                );
            }
        }
        self.saved_active_id = active_id.clone();
        Ok((active_id, sessions))
    }

    pub fn save(&mut self, active_id: &str, sessions: &[ChatSession]) -> StorageResult<()> {
        let mut current_sessions = HashMap::new();
        let mut current_messages = HashMap::new();
        let transaction = self.connection.transaction()?;

        for (position, session) in sessions.iter().enumerate() {
            let signature = session_signature(session, position)?;
            if self.saved_sessions.get(&session.id) != Some(&signature) {
                let params_json = match &session.params {
                    Some(params) => serde_json::to_string(params)?,
                    None => String::new(),
                };
                let tools_json = match &session.tools {
                    Some(tools) => serde_json::to_string(tools)?,
                    None => String::new(),
                };
                transaction.execute(
                    "INSERT INTO sessions (
                        id, title, folder, model, provider_id, created_at, updated_at, position,
                        pinned, favorite, title_auto, params, tools
                     )
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                     ON CONFLICT(id) DO UPDATE SET
                         title = excluded.title, folder = excluded.folder,
                         model = excluded.model, provider_id = excluded.provider_id,
                         created_at = excluded.created_at,
                         updated_at = excluded.updated_at, position = excluded.position,
                         pinned = excluded.pinned, favorite = excluded.favorite,
                         title_auto = excluded.title_auto, params = excluded.params,
                         tools = excluded.tools",
                    params![
                        session.id,
                        session.title,
                        session.folder,
                        session.model,
                        session.provider_id,
                        session.created_at,
                        session.updated_at,
                        position as i64,
                        session.pinned as i64,
                        session.favorite as i64,
                        session.title_auto as i64,
                        params_json,
                        tools_json
                    ],
                )?;
            }
            current_sessions.insert(session.id.clone(), signature);

            for (message_position, message) in session.messages.iter().enumerate() {
                let signature = message_signature(&session.id, message, message_position)?;
                if self.saved_messages.get(&message.id) != Some(&signature) {
                    transaction.execute(
                        "INSERT INTO messages (id, session_id, position, content, payload)
                         VALUES (?1, ?2, ?3, ?4, ?5)
                         ON CONFLICT(id) DO UPDATE SET
                             session_id = excluded.session_id, position = excluded.position,
                             content = excluded.content, payload = excluded.payload",
                        params![
                            message.id,
                            session.id,
                            message_position as i64,
                            message.content,
                            serde_json::to_string(message)?
                        ],
                    )?;
                }
                current_messages.insert(message.id.clone(), signature);
            }
        }

        for id in self.saved_messages.keys() {
            if !current_messages.contains_key(id) {
                transaction.execute("DELETE FROM messages WHERE id = ?1", [id])?;
            }
        }
        for id in self.saved_sessions.keys() {
            if !current_sessions.contains_key(id) {
                transaction.execute("DELETE FROM sessions WHERE id = ?1", [id])?;
            }
        }
        if self.saved_active_id != active_id {
            transaction.execute(
                "INSERT INTO metadata (key, value) VALUES ('active_session_id', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [active_id],
            )?;
        }
        transaction.commit()?;
        self.saved_sessions = current_sessions;
        self.saved_messages = current_messages;
        self.saved_active_id = active_id.to_string();
        Ok(())
    }

    pub fn search_session_ids(&self, query: &str) -> StorageResult<HashSet<String>> {
        let escaped = query.replace('!', "!!").replace('%', "!%").replace('_', "!_");
        let pattern = format!("%{escaped}%");
        let mut stmt = self.connection.prepare(
            "SELECT sessions.id FROM sessions
             WHERE sessions.title LIKE ?1 ESCAPE '!'
                OR EXISTS (
                    SELECT 1 FROM messages
                    WHERE messages.session_id = sessions.id
                      AND messages.content LIKE ?1 ESCAPE '!'
                )",
        )?;
        let rows = stmt.query_map([pattern], |row| row.get::<_, String>(0))?;
        let mut ids = HashSet::new();
        for row in rows {
            ids.insert(row?);
        }
        Ok(ids)
    }
}

fn migrate(connection: &Connection) -> StorageResult<()> {
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version > 4 {
        return Err(format!("Unsupported database version: {version}").into());
    }
    if version == 0 {
        connection.execute_batch(
            "BEGIN;
             CREATE TABLE IF NOT EXISTS metadata (
                 key TEXT PRIMARY KEY,
                 value TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS sessions (
                 id TEXT PRIMARY KEY,
                 title TEXT NOT NULL,
                 folder TEXT NOT NULL,
                 model TEXT NOT NULL,
                 provider_id TEXT NOT NULL DEFAULT '',
                 created_at TEXT NOT NULL,
                 updated_at TEXT NOT NULL,
                 position INTEGER NOT NULL,
                 pinned INTEGER NOT NULL DEFAULT 0,
                 favorite INTEGER NOT NULL DEFAULT 0,
                 title_auto INTEGER NOT NULL DEFAULT 0,
                 params TEXT NOT NULL DEFAULT '',
                 tools TEXT NOT NULL DEFAULT ''
             );
             CREATE TABLE IF NOT EXISTS messages (
                 id TEXT PRIMARY KEY,
                 session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                 position INTEGER NOT NULL,
                 content TEXT NOT NULL,
                 payload TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS messages_session_order
                 ON messages(session_id, position);
             PRAGMA user_version = 4;
             COMMIT;",
        )?;
        return Ok(());
    }
    if version == 1 {
        connection.execute(
            "ALTER TABLE sessions ADD COLUMN provider_id TEXT NOT NULL DEFAULT ''",
            [],
        )?;
    }
    if version < 3 {
        connection.execute_batch(
            "BEGIN;
             ALTER TABLE sessions ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;
             ALTER TABLE sessions ADD COLUMN favorite INTEGER NOT NULL DEFAULT 0;
             ALTER TABLE sessions ADD COLUMN title_auto INTEGER NOT NULL DEFAULT 0;
             ALTER TABLE sessions ADD COLUMN params TEXT NOT NULL DEFAULT '';
             PRAGMA user_version = 3;
             COMMIT;",
        )?;
    }
    if version < 4 {
        // 空串 = 没设过 = chat 模式。老会话升级上来一律是 chat，
        // 也就是「升级后不会突然自己调工具」——工具调用有副作用，默认不开更安全
        connection.execute_batch(
            "BEGIN;
             ALTER TABLE sessions ADD COLUMN tools TEXT NOT NULL DEFAULT '';
             PRAGMA user_version = 4;
             COMMIT;",
        )?;
    }
    Ok(())
}

fn session_signature(session: &ChatSession, position: usize) -> StorageResult<String> {
    Ok(serde_json::to_string(&(
        &session.title,
        &session.folder,
        &session.model,
        &session.provider_id,
        &session.created_at,
        &session.updated_at,
        &session.pinned,
        &session.favorite,
        &session.title_auto,
        &session.params,
        &session.tools,
        position,
    ))?)
}

fn message_signature(session_id: &str, message: &ChatMessage, position: usize) -> StorageResult<String> {
    Ok(serde_json::to_string(&(session_id, position, message))?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ReasoningLevel;
    use tempfile::tempdir;

    #[test]
    fn upgrades_v1_database_without_losing_sessions() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("chats.db");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE sessions (
                 id TEXT PRIMARY KEY, title TEXT NOT NULL, folder TEXT NOT NULL,
                 model TEXT NOT NULL, created_at TEXT NOT NULL,
                 updated_at TEXT NOT NULL, position INTEGER NOT NULL
             );
             CREATE TABLE messages (
                 id TEXT PRIMARY KEY, session_id TEXT NOT NULL,
                 position INTEGER NOT NULL, content TEXT NOT NULL, payload TEXT NOT NULL
             );
             INSERT INTO sessions VALUES ('session-1', 'Old', 'Default', 'model', '2026-09-25', '2026-09-25', 0);
             PRAGMA user_version = 1;",
            )
            .unwrap();
        drop(connection);

        let mut database = Database::open(&path).unwrap();
        let (_, sessions) = database.load().unwrap();
        assert_eq!(sessions[0].id, "session-1");
        assert!(sessions[0].provider_id.is_empty());
        assert!(!sessions[0].pinned);
        // 老库升级上来的会话没设过工具，一律按对话处理（不会突然自己调工具）
        assert!(sessions[0].tools.is_none());
        assert!(!sessions[0].is_agent());
        assert!(sessions[0].tool_sources().is_empty());
        let version: i64 = database
            .connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 4);
    }

    #[test]
    fn upgrades_v2_and_roundtrips_session_params() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("chats.db");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE sessions (
                 id TEXT PRIMARY KEY, title TEXT NOT NULL, folder TEXT NOT NULL,
                 model TEXT NOT NULL, provider_id TEXT NOT NULL DEFAULT '',
                 created_at TEXT NOT NULL, updated_at TEXT NOT NULL, position INTEGER NOT NULL
             );
             CREATE TABLE messages (
                 id TEXT PRIMARY KEY, session_id TEXT NOT NULL,
                 position INTEGER NOT NULL, content TEXT NOT NULL, payload TEXT NOT NULL
             );
             PRAGMA user_version = 2;",
            )
            .unwrap();
        drop(connection);

        let mut database = Database::open(&path).unwrap();
        let mut session = ChatSession::new("Pinned".into(), "工作".into(), "model".into(), "provider".into());
        session.pinned = true;
        session.favorite = true;
        session.title_auto = false;
        session.params = Some(ChatParams {
            temperature: Some(0.2),
            reasoning: Some(ReasoningLevel::Low),
            ..ChatParams::default()
        });
        database.save(&session.id, &[session.clone()]).unwrap();
        drop(database);

        let mut database = Database::open(&path).unwrap();
        let (active_id, sessions) = database.load().unwrap();
        assert_eq!(active_id, session.id);
        assert!(sessions[0].pinned && sessions[0].favorite);
        assert!(!sessions[0].title_auto);
        assert_eq!(sessions[0].params.as_ref().unwrap().temperature, Some(0.2));
    }
}
