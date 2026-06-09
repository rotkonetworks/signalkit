use rusqlite::Row;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chat {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, rename = "profileName")]
    pub profile_name: Option<String>,
    #[serde(default, rename = "profileFullName")]
    pub profile_full_name: Option<String>,
    #[serde(default)]
    pub e164: Option<String>,
    #[serde(default, rename = "serviceId")]
    pub service_id: Option<String>,
    #[serde(default, rename = "groupId")]
    pub group_id: Option<String>,
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    #[serde(default)]
    pub active_at: Option<i64>,
    #[serde(default, skip_deserializing)]
    pub total_messages: i64,
}

impl Chat {
    pub fn stub(id: String) -> Self {
        Self {
            id,
            name: None,
            profile_name: None,
            profile_full_name: None,
            e164: None,
            service_id: None,
            group_id: None,
            kind: None,
            active_at: None,
            total_messages: 0,
        }
    }

    pub fn display_name(&self) -> &str {
        self.name
            .as_deref()
            .or(self.profile_full_name.as_deref())
            .or(self.profile_name.as_deref())
            .or(self.e164.as_deref())
            .unwrap_or("Unknown")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageRow {
    pub id: String,
    pub conversation_id: String,
    pub sent_at: Option<i64>,
    pub received_at: Option<i64>,
    pub body: Option<String>,
    pub source: Option<String>,
    /// Signal Desktop's `type` column — "incoming", "outgoing", "keychange", etc.
    /// Renamed to `kind` on the wire so JS doesn't have to dodge the reserved word.
    pub kind: Option<String>,
    pub has_attachments: bool,
}

impl MessageRow {
    pub(crate) fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        let has_attachments: Option<i64> = row.get("hasAttachments")?;
        Ok(Self {
            id: row.get("id")?,
            conversation_id: row.get("conversationId")?,
            sent_at: row.get("sent_at")?,
            received_at: row.get("received_at")?,
            body: row.get("body")?,
            source: row.get("source")?,
            kind: row.get("type")?,
            has_attachments: has_attachments.unwrap_or(0) != 0,
        })
    }
}
