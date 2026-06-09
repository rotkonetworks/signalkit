use std::collections::HashMap;
use std::path::Path;

use rusqlite::{params, Connection, OpenFlags};

use crate::domain::{Chat, MessageRow};
use crate::Result;

pub struct DesktopDb {
    conn: Connection,
}

impl DesktopDb {
    pub fn open(db_path: &Path, hex_key: &str) -> Result<Self> {
        let conn = Connection::open_with_flags(
            db_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.pragma_update(None, "key", format!("x'{}'", hex_key))?;
        conn.pragma_update(None, "cipher_compatibility", 4)?;
        conn.query_row("SELECT count(*) FROM sqlite_master", [], |_| Ok(()))?;
        Ok(Self { conn })
    }

    pub fn list_chats(&self, include_empty: bool) -> Result<Vec<Chat>> {
        let sql = if include_empty {
            "SELECT id, json FROM conversations ORDER BY COALESCE(active_at, 0) DESC"
        } else {
            "SELECT id, json FROM conversations WHERE active_at IS NOT NULL ORDER BY active_at DESC"
        };
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let json: String = row.get(1)?;
            Ok((id, json))
        })?;
        let mut chats = Vec::new();
        for r in rows {
            let (id, json) = r?;
            let mut chat: Chat =
                serde_json::from_str(&json).unwrap_or_else(|_| Chat::stub(id.clone()));
            chat.id = id;
            chats.push(chat);
        }
        let mut counts: HashMap<String, i64> = HashMap::new();
        {
            let mut stmt = self
                .conn
                .prepare("SELECT conversationId, COUNT(*) FROM messages GROUP BY conversationId")?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?;
            for r in rows {
                let (k, v) = r?;
                counts.insert(k, v);
            }
        }
        for chat in &mut chats {
            chat.total_messages = counts.get(&chat.id).copied().unwrap_or(0);
        }
        Ok(chats)
    }

    pub fn find_chat_by_name(&self, name: &str) -> Result<Option<Chat>> {
        Ok(self
            .list_chats(false)?
            .into_iter()
            .find(|c| c.display_name().eq_ignore_ascii_case(name)))
    }

    pub fn get_messages(
        &self,
        conversation_id: &str,
        limit: Option<u32>,
        offset: u32,
        from_ms: Option<i64>,
        to_ms: Option<i64>,
    ) -> Result<Vec<MessageRow>> {
        let lim = limit.map(|l| l as i64).unwrap_or(-1);
        let from = from_ms.unwrap_or(i64::MIN);
        let to = to_ms.unwrap_or(i64::MAX);
        let mut stmt = self.conn.prepare(
            "SELECT id, conversationId, sent_at, received_at, body, source, type, hasAttachments
             FROM messages
             WHERE conversationId = ?1
               AND (sent_at IS NULL OR sent_at BETWEEN ?2 AND ?3)
             ORDER BY sent_at DESC
             LIMIT ?4 OFFSET ?5",
        )?;
        let rows = stmt.query_map(
            params![conversation_id, from, to, lim, offset as i64],
            MessageRow::from_row,
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn search_messages(
        &self,
        conversation_id: Option<&str>,
        query: &str,
        limit: Option<u32>,
        from_ms: Option<i64>,
        to_ms: Option<i64>,
    ) -> Result<Vec<MessageRow>> {
        let lim = limit.map(|l| l as i64).unwrap_or(-1);
        let from = from_ms.unwrap_or(i64::MIN);
        let to = to_ms.unwrap_or(i64::MAX);
        let pattern = format!("%{}%", query.replace('%', r"\%").replace('_', r"\_"));
        let rows = match conversation_id {
            Some(cid) => {
                let mut stmt = self.conn.prepare(
                    "SELECT id, conversationId, sent_at, received_at, body, source, type, hasAttachments
                     FROM messages
                     WHERE conversationId = ?1
                       AND body LIKE ?2 ESCAPE '\\'
                       AND (sent_at IS NULL OR sent_at BETWEEN ?3 AND ?4)
                     ORDER BY sent_at DESC
                     LIMIT ?5",
                )?;
                let mapped = stmt.query_map(
                    params![cid, pattern, from, to, lim],
                    MessageRow::from_row,
                )?;
                mapped.collect::<rusqlite::Result<Vec<_>>>()?
            }
            None => {
                let mut stmt = self.conn.prepare(
                    "SELECT id, conversationId, sent_at, received_at, body, source, type, hasAttachments
                     FROM messages
                     WHERE body LIKE ?1 ESCAPE '\\'
                       AND (sent_at IS NULL OR sent_at BETWEEN ?2 AND ?3)
                     ORDER BY sent_at DESC
                     LIMIT ?4",
                )?;
                let mapped = stmt.query_map(
                    params![pattern, from, to, lim],
                    MessageRow::from_row,
                )?;
                mapped.collect::<rusqlite::Result<Vec<_>>>()?
            }
        };
        Ok(rows)
    }
}
