//! Persistence: layer 1 (raw) and layer 2 (derived) in libSQL.
//! Layer 2 is rebuildable from layer 1; user decisions (merges, image
//! allowances) are keyed on addresses and survive rebuilds.

use crate::types::*;
use anyhow::{Context, Result};
use libsql::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone)]
pub struct Store {
    pub conn: Connection,
}

#[derive(Debug, Clone, Serialize)]
pub struct Account {
    pub id: i64,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    #[serde(skip)]
    pub password_enc: Vec<u8>,
    pub excluded_folders: Vec<String>,
    pub sync_interval_mins: i64,
    pub last_sync_at: Option<i64>,
    pub last_sync_status: Option<String>,
    /// "imap" or a fixture directory path (for tests / demo)
    pub kind: String,
    /// "ssl" (implicit TLS) or "starttls"
    pub security: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredMessage {
    pub id: i64,
    pub account_id: i64,
    pub identity: String,
    pub msgid: Option<String>,
    pub subject: String,
    pub subject_norm: String,
    pub from: Vec<Addr>,
    pub to: Vec<Addr>,
    pub cc: Vec<Addr>,
    pub date_canonical: i64,
    pub date_source: String,
    pub date_offset_mins: i32,
    pub received_top: Option<i64>,
    pub date_hdr: Option<i64>,
    pub internaldate: Option<i64>,
    pub skew: bool,
    pub thread_id: String,
    pub references: Vec<String>,
    pub body_text: String,
    pub spans: Vec<Span>,
    pub has_html: bool,
    pub has_attach: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct StoredPart {
    pub path: String,
    pub kind: String,
    pub mime: String,
    pub filename: Option<String>,
    pub size: u64,
    pub content_id: Option<String>,
    pub inline: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct StoredLocation {
    pub folder: String,
    pub uid: u32,
    pub uidvalidity: u32,
    pub internaldate: Option<i64>,
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS accounts (
  id INTEGER PRIMARY KEY,
  name TEXT UNIQUE NOT NULL,
  kind TEXT NOT NULL DEFAULT 'imap',
  host TEXT NOT NULL DEFAULT '',
  port INTEGER NOT NULL DEFAULT 993,
  username TEXT NOT NULL DEFAULT '',
  password_enc BLOB NOT NULL DEFAULT x'',
  excluded_folders TEXT NOT NULL DEFAULT '[]',
  sync_interval_mins INTEGER NOT NULL DEFAULT 60,
  last_sync_at INTEGER,
  last_sync_status TEXT,
  created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS messages (
  id INTEGER PRIMARY KEY,
  account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  identity TEXT NOT NULL,
  msgid TEXT,
  content_hash TEXT NOT NULL DEFAULT '',
  raw_headers BLOB NOT NULL,
  flags TEXT NOT NULL DEFAULT '[]',
  subject TEXT NOT NULL DEFAULT '',
  subject_norm TEXT NOT NULL DEFAULT '',
  from_json TEXT NOT NULL DEFAULT '[]',
  to_json TEXT NOT NULL DEFAULT '[]',
  cc_json TEXT NOT NULL DEFAULT '[]',
  date_canonical INTEGER NOT NULL DEFAULT 0,
  date_source TEXT NOT NULL DEFAULT 'none',
  date_offset_mins INTEGER NOT NULL DEFAULT 0,
  received_top INTEGER,
  date_hdr INTEGER,
  internaldate INTEGER,
  skew INTEGER NOT NULL DEFAULT 0,
  thread_id TEXT NOT NULL DEFAULT '',
  refs_json TEXT NOT NULL DEFAULT '[]',
  body_text TEXT NOT NULL DEFAULT '',
  spans_json TEXT NOT NULL DEFAULT '[]',
  has_html INTEGER NOT NULL DEFAULT 0,
  has_attach INTEGER NOT NULL DEFAULT 0,
  ingested_at INTEGER NOT NULL,
  UNIQUE(account_id, identity)
);
CREATE INDEX IF NOT EXISTS idx_messages_thread ON messages(account_id, thread_id);
CREATE INDEX IF NOT EXISTS idx_messages_msgid ON messages(account_id, msgid);
CREATE TABLE IF NOT EXISTS parts (
  id INTEGER PRIMARY KEY,
  message_id INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
  path TEXT NOT NULL,
  kind TEXT NOT NULL,
  mime TEXT NOT NULL DEFAULT '',
  filename TEXT,
  size INTEGER NOT NULL DEFAULT 0,
  content_id TEXT,
  inline_flag INTEGER NOT NULL DEFAULT 0,
  content BLOB
);
CREATE INDEX IF NOT EXISTS idx_parts_msg ON parts(message_id);
CREATE TABLE IF NOT EXISTS locations (
  account_id INTEGER NOT NULL,
  message_id INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
  folder TEXT NOT NULL,
  uid INTEGER NOT NULL,
  uidvalidity INTEGER NOT NULL,
  flags TEXT NOT NULL DEFAULT '[]',
  internaldate INTEGER,
  PRIMARY KEY (message_id, folder)
);
CREATE INDEX IF NOT EXISTS idx_locations_af ON locations(account_id, folder, uid);
CREATE TABLE IF NOT EXISTS contacts (
  account_id INTEGER NOT NULL,
  email TEXT NOT NULL,
  names_json TEXT NOT NULL DEFAULT '{}',
  org TEXT,
  is_role INTEGER NOT NULL DEFAULT 0,
  msg_count INTEGER NOT NULL DEFAULT 0,
  last_seen INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (account_id, email)
);
CREATE TABLE IF NOT EXISTS merge_ops (
  id INTEGER PRIMARY KEY,
  account_id INTEGER NOT NULL,
  op TEXT NOT NULL,
  addr_a TEXT NOT NULL,
  addr_b TEXT NOT NULL,
  at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS image_allow (
  account_id INTEGER NOT NULL,
  sender TEXT NOT NULL,
  at INTEGER NOT NULL,
  PRIMARY KEY (account_id, sender)
);
CREATE TABLE IF NOT EXISTS folder_state (
  account_id INTEGER NOT NULL,
  folder TEXT NOT NULL,
  uidvalidity INTEGER NOT NULL DEFAULT 0,
  updated_at INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (account_id, folder)
);
CREATE TABLE IF NOT EXISTS failures (
  id INTEGER PRIMARY KEY,
  account_id INTEGER NOT NULL,
  folder TEXT NOT NULL DEFAULT '',
  uid INTEGER,
  error TEXT NOT NULL,
  raw BLOB,
  at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS sync_log (
  id INTEGER PRIMARY KEY,
  account_id INTEGER NOT NULL,
  started_at INTEGER NOT NULL,
  finished_at INTEGER,
  new_msgs INTEGER NOT NULL DEFAULT 0,
  removed INTEGER NOT NULL DEFAULT 0,
  failed INTEGER NOT NULL DEFAULT 0,
  status TEXT NOT NULL DEFAULT 'running',
  detail TEXT
);
"#;

impl Store {
    pub async fn open(path: &str) -> Result<Self> {
        let db = libsql::Builder::new_local(path).build().await?;
        let conn = db.connect()?;
        conn.execute_batch(SCHEMA).await?;
        // additive migrations; failure means the column already exists
        conn.execute(
            "ALTER TABLE accounts ADD COLUMN security TEXT NOT NULL DEFAULT 'ssl'",
            (),
        )
        .await
        .ok();
        conn.execute("PRAGMA journal_mode=WAL", ()).await.ok();
        conn.execute("PRAGMA foreign_keys=ON", ()).await.ok();
        Ok(Self { conn })
    }

    // ---------- accounts ----------

    #[allow(clippy::too_many_arguments)]
    pub async fn add_account(
        &self,
        name: &str,
        kind: &str,
        host: &str,
        port: u16,
        username: &str,
        password_enc: &[u8],
        security: &str,
    ) -> Result<i64> {
        self.conn
            .execute(
                "INSERT INTO accounts (name, kind, host, port, username, password_enc, security, created_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                params![name, kind, host, port as i64, username, password_enc, security, now()],
            )
            .await?;
        Ok(self.conn.last_insert_rowid())
    }

    pub async fn accounts(&self) -> Result<Vec<Account>> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, name, kind, host, port, username, password_enc, excluded_folders,
                        sync_interval_mins, last_sync_at, last_sync_status, security
                 FROM accounts ORDER BY id",
                (),
            )
            .await?;
        let mut out = Vec::new();
        while let Some(r) = rows.next().await? {
            out.push(Account {
                id: r.get(0)?,
                name: r.get(1)?,
                kind: r.get(2)?,
                host: r.get(3)?,
                port: r.get::<i64>(4)? as u16,
                username: r.get(5)?,
                password_enc: r.get::<Vec<u8>>(6)?,
                excluded_folders: serde_json::from_str(&r.get::<String>(7)?)?,
                sync_interval_mins: r.get(8)?,
                last_sync_at: r.get(9)?,
                last_sync_status: r.get(10)?,
                security: r.get(11)?,
            });
        }
        Ok(out)
    }

    pub async fn account(&self, id: i64) -> Result<Account> {
        self.accounts()
            .await?
            .into_iter()
            .find(|a| a.id == id)
            .context("no such account")
    }

    pub async fn set_excluded_folders(&self, id: i64, folders: &[String]) -> Result<()> {
        self.conn
            .execute(
                "UPDATE accounts SET excluded_folders = ? WHERE id = ?",
                params![serde_json::to_string(folders)?, id],
            )
            .await?;
        Ok(())
    }

    pub async fn set_sync_interval(&self, id: i64, mins: i64) -> Result<()> {
        self.conn
            .execute(
                "UPDATE accounts SET sync_interval_mins = ? WHERE id = ?",
                params![mins, id],
            )
            .await?;
        Ok(())
    }

    pub async fn set_sync_result(&self, id: i64, status: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE accounts SET last_sync_at = ?, last_sync_status = ? WHERE id = ?",
                params![now(), status, id],
            )
            .await?;
        Ok(())
    }

    /// Remove an account and every row derived from it.
    pub async fn delete_account(&self, id: i64) -> Result<()> {
        for sql in [
            "DELETE FROM locations WHERE account_id = ?",
            "DELETE FROM parts WHERE message_id IN (SELECT id FROM messages WHERE account_id = ?)",
            "DELETE FROM messages WHERE account_id = ?",
            "DELETE FROM contacts WHERE account_id = ?",
            "DELETE FROM merge_ops WHERE account_id = ?",
            "DELETE FROM image_allow WHERE account_id = ?",
            "DELETE FROM folder_state WHERE account_id = ?",
            "DELETE FROM failures WHERE account_id = ?",
            "DELETE FROM sync_log WHERE account_id = ?",
            "DELETE FROM accounts WHERE id = ?",
        ] {
            self.conn.execute(sql, params![id]).await?;
        }
        Ok(())
    }

    // ---------- messages ----------

    /// Insert a message if its identity is new; either way ensure the location
    /// exists. Returns (db_id, was_new).
    pub async fn upsert_message(
        &self,
        account_id: i64,
        n: &NormalizedMessage,
        raw: &RawMessage,
        text_parts_mime: &[(String, String)], // (path, mime) for stored text parts
        loc: &StoredLocation,
    ) -> Result<(i64, bool)> {
        let existing: Option<i64> = {
            let mut rows = self
                .conn
                .query(
                    "SELECT id FROM messages WHERE account_id = ? AND identity = ?",
                    params![account_id, n.identity.clone()],
                )
                .await?;
            match rows.next().await? {
                Some(r) => Some(r.get(0)?),
                None => None,
            }
        };
        let (id, was_new) = match existing {
            Some(id) => (id, false),
            None => {
                self.conn
                    .execute(
                        "INSERT INTO messages (account_id, identity, msgid, content_hash, raw_headers,
                            flags, subject, subject_norm, from_json, to_json, cc_json,
                            date_canonical, date_source, date_offset_mins, received_top, date_hdr,
                            internaldate, skew, refs_json, body_text, spans_json, has_html,
                            has_attach, ingested_at)
                         VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
                        params![
                            account_id,
                            n.identity.clone(),
                            n.msgid.clone(),
                            n.content_hash.clone(),
                            raw.header_bytes.clone(),
                            serde_json::to_string(&raw.flags)?,
                            n.subject.clone(),
                            n.subject_norm.clone(),
                            serde_json::to_string(&n.from)?,
                            serde_json::to_string(&n.to)?,
                            serde_json::to_string(&n.cc)?,
                            n.date.canonical,
                            n.date.source.as_str(),
                            n.date.offset_mins as i64,
                            n.date.received_top,
                            n.date.date_hdr,
                            n.date.internaldate,
                            n.date.skew as i64,
                            serde_json::to_string(&n.references)?,
                            n.body_text.clone(),
                            serde_json::to_string(&n.spans)?,
                            n.has_html as i64,
                            (!n.attachments.is_empty()) as i64,
                            now()
                        ],
                    )
                    .await?;
                let id = self.conn.last_insert_rowid();
                for tp in &raw.text_parts {
                    let mime = text_parts_mime
                        .iter()
                        .find(|(p, _)| *p == tp.path)
                        .map(|(_, m)| m.clone())
                        .unwrap_or_else(|| "text/plain".into());
                    self.conn
                        .execute(
                            "INSERT INTO parts (message_id, path, kind, mime, size, content)
                             VALUES (?, ?, 'text', ?, ?, ?)",
                            params![id, tp.path.clone(), mime, tp.entity.len() as i64, tp.entity.clone()],
                        )
                        .await?;
                }
                for a in &n.attachments {
                    self.conn
                        .execute(
                            "INSERT INTO parts (message_id, path, kind, mime, filename, size, content_id, inline_flag)
                             VALUES (?, ?, 'attach', ?, ?, ?, ?, ?)",
                            params![
                                id,
                                a.path.clone(),
                                a.mime.clone(),
                                a.filename.clone(),
                                a.size as i64,
                                a.content_id.clone(),
                                a.inline as i64
                            ],
                        )
                        .await?;
                }
                (id, true)
            }
        };
        self.conn
            .execute(
                "INSERT INTO locations (account_id, message_id, folder, uid, uidvalidity, internaldate)
                 VALUES (?,?,?,?,?,?)
                 ON CONFLICT(message_id, folder) DO UPDATE SET
                   uid = excluded.uid, uidvalidity = excluded.uidvalidity,
                   internaldate = excluded.internaldate",
                params![account_id, id, loc.folder.clone(), loc.uid as i64, loc.uidvalidity as i64, loc.internaldate],
            )
            .await?;
        Ok((id, was_new))
    }

    pub async fn message(&self, id: i64) -> Result<Option<StoredMessage>> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, account_id, identity, msgid, subject, subject_norm, from_json, to_json,
                        cc_json, date_canonical, date_source, date_offset_mins, received_top,
                        date_hdr, internaldate, skew, thread_id, refs_json, body_text, spans_json,
                        has_html, has_attach
                 FROM messages WHERE id = ?",
                params![id],
            )
            .await?;
        match rows.next().await? {
            Some(r) => Ok(Some(row_to_message(&r)?)),
            None => Ok(None),
        }
    }

    pub async fn messages_by_ids(&self, ids: &[i64]) -> Result<Vec<StoredMessage>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT id, account_id, identity, msgid, subject, subject_norm, from_json, to_json,
                    cc_json, date_canonical, date_source, date_offset_mins, received_top,
                    date_hdr, internaldate, skew, thread_id, refs_json, body_text, spans_json,
                    has_html, has_attach
             FROM messages WHERE id IN ({placeholders})"
        );
        let params_vec: Vec<libsql::Value> = ids.iter().map(|i| (*i).into()).collect();
        let mut rows = self.conn.query(&sql, params_vec).await?;
        let mut out = Vec::new();
        while let Some(r) = rows.next().await? {
            out.push(row_to_message(&r)?);
        }
        // preserve requested order
        let pos: HashMap<i64, usize> = ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
        out.sort_by_key(|m| pos.get(&m.id).copied().unwrap_or(usize::MAX));
        Ok(out)
    }

    pub async fn thread_messages(&self, account_id: i64, thread_id: &str) -> Result<Vec<StoredMessage>> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, account_id, identity, msgid, subject, subject_norm, from_json, to_json,
                        cc_json, date_canonical, date_source, date_offset_mins, received_top,
                        date_hdr, internaldate, skew, thread_id, refs_json, body_text, spans_json,
                        has_html, has_attach
                 FROM messages WHERE account_id = ? AND thread_id = ? ORDER BY date_canonical",
                params![account_id, thread_id],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(r) = rows.next().await? {
            out.push(row_to_message(&r)?);
        }
        Ok(out)
    }

    pub async fn thread_sizes(&self, account_id: i64, thread_ids: &[String]) -> Result<HashMap<String, i64>> {
        let mut out = HashMap::new();
        if thread_ids.is_empty() {
            return Ok(out);
        }
        let placeholders = thread_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT thread_id, COUNT(*) FROM messages
             WHERE account_id = ? AND thread_id IN ({placeholders}) GROUP BY thread_id"
        );
        let mut params_vec: Vec<libsql::Value> = vec![account_id.into()];
        params_vec.extend(thread_ids.iter().map(|t| t.clone().into()));
        let mut rows = self.conn.query(&sql, params_vec).await?;
        while let Some(r) = rows.next().await? {
            out.insert(r.get::<String>(0)?, r.get::<i64>(1)?);
        }
        Ok(out)
    }

    pub async fn raw_headers(&self, id: i64) -> Result<Vec<u8>> {
        let mut rows = self
            .conn
            .query("SELECT raw_headers FROM messages WHERE id = ?", params![id])
            .await?;
        let r = rows.next().await?.context("no such message")?;
        Ok(r.get::<Vec<u8>>(0)?)
    }

    pub async fn parts(&self, message_id: i64) -> Result<Vec<StoredPart>> {
        let mut rows = self
            .conn
            .query(
                "SELECT path, kind, mime, filename, size, content_id, inline_flag
                 FROM parts WHERE message_id = ? ORDER BY id",
                params![message_id],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(r) = rows.next().await? {
            out.push(StoredPart {
                path: r.get(0)?,
                kind: r.get(1)?,
                mime: r.get(2)?,
                filename: r.get(3)?,
                size: r.get::<i64>(4)? as u64,
                content_id: r.get(5)?,
                inline: r.get::<i64>(6)? != 0,
            });
        }
        Ok(out)
    }

    pub async fn text_part_content(&self, message_id: i64, path: &str) -> Result<Option<Vec<u8>>> {
        let mut rows = self
            .conn
            .query(
                "SELECT content FROM parts WHERE message_id = ? AND path = ? AND kind = 'text'",
                params![message_id, path],
            )
            .await?;
        match rows.next().await? {
            Some(r) => Ok(r.get(0)?),
            None => Ok(None),
        }
    }

    pub async fn locations(&self, message_id: i64) -> Result<Vec<StoredLocation>> {
        let mut rows = self
            .conn
            .query(
                "SELECT folder, uid, uidvalidity, internaldate FROM locations WHERE message_id = ?",
                params![message_id],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(r) = rows.next().await? {
            out.push(StoredLocation {
                folder: r.get(0)?,
                uid: r.get::<i64>(1)? as u32,
                uidvalidity: r.get::<i64>(2)? as u32,
                internaldate: r.get(3)?,
            });
        }
        Ok(out)
    }

    pub async fn update_location(
        &self,
        message_id: i64,
        account_id: i64,
        loc: &StoredLocation,
    ) -> Result<()> {
        // A relocation replaces all stale pointers with the fresh one.
        self.conn
            .execute("DELETE FROM locations WHERE message_id = ?", params![message_id])
            .await?;
        self.conn
            .execute(
                "INSERT INTO locations (account_id, message_id, folder, uid, uidvalidity, internaldate)
                 VALUES (?,?,?,?,?,?)",
                params![account_id, message_id, loc.folder.clone(), loc.uid as i64, loc.uidvalidity as i64, loc.internaldate],
            )
            .await?;
        Ok(())
    }

    /// UIDs currently known for a folder -> message_id.
    pub async fn folder_uids(&self, account_id: i64, folder: &str) -> Result<HashMap<u32, i64>> {
        let mut rows = self
            .conn
            .query(
                "SELECT uid, message_id FROM locations WHERE account_id = ? AND folder = ?",
                params![account_id, folder],
            )
            .await?;
        let mut out = HashMap::new();
        while let Some(r) = rows.next().await? {
            out.insert(r.get::<i64>(0)? as u32, r.get::<i64>(1)?);
        }
        Ok(out)
    }

    /// Remove one location; if the message has no locations left, delete it
    /// entirely and report its id so the index entry can be removed too.
    pub async fn remove_location(&self, account_id: i64, folder: &str, uid: u32) -> Result<Option<i64>> {
        let mut rows = self
            .conn
            .query(
                "SELECT message_id FROM locations WHERE account_id = ? AND folder = ? AND uid = ?",
                params![account_id, folder, uid as i64],
            )
            .await?;
        let Some(r) = rows.next().await? else {
            return Ok(None);
        };
        let mid: i64 = r.get(0)?;
        self.conn
            .execute(
                "DELETE FROM locations WHERE account_id = ? AND folder = ? AND uid = ?",
                params![account_id, folder, uid as i64],
            )
            .await?;
        let mut rows = self
            .conn
            .query("SELECT COUNT(*) FROM locations WHERE message_id = ?", params![mid])
            .await?;
        let cnt: i64 = rows.next().await?.context("count")?.get(0)?;
        if cnt == 0 {
            self.conn.execute("DELETE FROM parts WHERE message_id = ?", params![mid]).await?;
            self.conn.execute("DELETE FROM messages WHERE id = ?", params![mid]).await?;
            Ok(Some(mid))
        } else {
            Ok(None)
        }
    }

    /// Every folder that still has stored locations for this account.
    pub async fn located_folders(&self, account_id: i64) -> Result<Vec<String>> {
        let mut rows = self
            .conn
            .query(
                "SELECT DISTINCT folder FROM locations WHERE account_id = ?",
                params![account_id],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(r) = rows.next().await? {
            out.push(r.get(0)?);
        }
        Ok(out)
    }

    pub async fn delete_folder_state(&self, account_id: i64, folder: &str) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM folder_state WHERE account_id = ? AND folder = ?",
                params![account_id, folder],
            )
            .await?;
        Ok(())
    }

    pub async fn folder_uidvalidity(&self, account_id: i64, folder: &str) -> Result<Option<u32>> {
        let mut rows = self
            .conn
            .query(
                "SELECT uidvalidity FROM folder_state WHERE account_id = ? AND folder = ?",
                params![account_id, folder],
            )
            .await?;
        match rows.next().await? {
            Some(r) => Ok(Some(r.get::<i64>(0)? as u32)),
            None => Ok(None),
        }
    }

    pub async fn set_folder_uidvalidity(&self, account_id: i64, folder: &str, v: u32) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO folder_state (account_id, folder, uidvalidity, updated_at)
                 VALUES (?,?,?,?)
                 ON CONFLICT(account_id, folder) DO UPDATE SET uidvalidity = excluded.uidvalidity,
                   updated_at = excluded.updated_at",
                params![account_id, folder, v as i64, now()],
            )
            .await?;
        Ok(())
    }

    /// Drop all locations for a folder (UIDVALIDITY change). Messages that
    /// lose their last location are deleted; their ids are returned.
    pub async fn invalidate_folder(&self, account_id: i64, folder: &str) -> Result<Vec<i64>> {
        let uids: Vec<u32> = self.folder_uids(account_id, folder).await?.into_keys().collect();
        let mut deleted = Vec::new();
        for uid in uids {
            if let Some(id) = self.remove_location(account_id, folder, uid).await? {
                deleted.push(id);
            }
        }
        Ok(deleted)
    }

    // ---------- failures / sync log ----------

    pub async fn record_failure(
        &self,
        account_id: i64,
        folder: &str,
        uid: Option<u32>,
        error: &str,
        raw: Option<&[u8]>,
    ) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO failures (account_id, folder, uid, error, raw, at) VALUES (?,?,?,?,?,?)",
                params![account_id, folder, uid.map(|u| u as i64), error, raw.map(|r| r.to_vec()), now()],
            )
            .await?;
        Ok(())
    }

    pub async fn start_sync_log(&self, account_id: i64) -> Result<i64> {
        self.conn
            .execute(
                "INSERT INTO sync_log (account_id, started_at) VALUES (?, ?)",
                params![account_id, now()],
            )
            .await?;
        Ok(self.conn.last_insert_rowid())
    }

    pub async fn finish_sync_log(
        &self,
        id: i64,
        new_msgs: i64,
        removed: i64,
        failed: i64,
        status: &str,
        detail: &str,
    ) -> Result<()> {
        self.conn
            .execute(
                "UPDATE sync_log SET finished_at = ?, new_msgs = ?, removed = ?, failed = ?,
                        status = ?, detail = ? WHERE id = ?",
                params![now(), new_msgs, removed, failed, status, detail, id],
            )
            .await?;
        Ok(())
    }

    pub async fn sync_logs(&self, account_id: i64, limit: i64) -> Result<Vec<serde_json::Value>> {
        let mut rows = self
            .conn
            .query(
                "SELECT started_at, finished_at, new_msgs, removed, failed, status, detail
                 FROM sync_log WHERE account_id = ? ORDER BY id DESC LIMIT ?",
                params![account_id, limit],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(r) = rows.next().await? {
            out.push(serde_json::json!({
                "started_at": r.get::<i64>(0)?,
                "finished_at": r.get::<Option<i64>>(1)?,
                "new": r.get::<i64>(2)?,
                "removed": r.get::<i64>(3)?,
                "failed": r.get::<i64>(4)?,
                "status": r.get::<String>(5)?,
                "detail": r.get::<Option<String>>(6)?,
            }));
        }
        Ok(out)
    }

    pub async fn message_count(&self, account_id: i64) -> Result<i64> {
        let mut rows = self
            .conn
            .query("SELECT COUNT(*) FROM messages WHERE account_id = ?", params![account_id])
            .await?;
        Ok(rows.next().await?.context("count")?.get(0)?)
    }

    // ---------- threading (layer 2, recomputed) ----------

    /// Recompute thread membership for an account with union-find over
    /// message identities and their references. Returns ids whose thread_id
    /// changed (they need reindexing).
    /// ponytail: full recompute each sync, O(n alpha(n)); incremental unioning
    /// if account size ever makes this measurable.
    pub async fn recompute_threads(&self, account_id: i64) -> Result<Vec<i64>> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, identity, refs_json, thread_id FROM messages WHERE account_id = ?",
                params![account_id],
            )
            .await?;
        struct Row {
            id: i64,
            identity: String,
            refs: Vec<String>,
            old_thread: String,
        }
        let mut msgs = Vec::new();
        while let Some(r) = rows.next().await? {
            msgs.push(Row {
                id: r.get(0)?,
                identity: r.get(1)?,
                refs: serde_json::from_str(&r.get::<String>(2)?)?,
                old_thread: r.get(3)?,
            });
        }
        // union-find over identity strings (including referenced-but-absent ids,
        // which is what makes JWZ handle missing intermediates)
        let mut idx: HashMap<String, usize> = HashMap::new();
        let mut parent: Vec<usize> = Vec::new();
        fn find(parent: &mut Vec<usize>, mut x: usize) -> usize {
            while parent[x] != x {
                parent[x] = parent[parent[x]];
                x = parent[x];
            }
            x
        }
        fn intern(s: &str, idx: &mut HashMap<String, usize>, parent: &mut Vec<usize>) -> usize {
            if let Some(&i) = idx.get(s) {
                return i;
            }
            let i = parent.len();
            parent.push(i);
            idx.insert(s.to_string(), i);
            i
        }
        for m in &msgs {
            let a = intern(&m.identity, &mut idx, &mut parent);
            for r in &m.refs {
                let b = intern(r, &mut idx, &mut parent);
                let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
                if ra != rb {
                    parent[ra] = rb;
                }
            }
        }
        // thread id = lexicographically smallest member identity of the group
        let mut group_min: HashMap<usize, String> = HashMap::new();
        for m in &msgs {
            let root = find(&mut parent, idx[m.identity.as_str()]);
            group_min
                .entry(root)
                .and_modify(|cur| {
                    if m.identity < *cur {
                        *cur = m.identity.clone();
                    }
                })
                .or_insert_with(|| m.identity.clone());
        }
        let mut changed = Vec::new();
        for m in &msgs {
            let root = find(&mut parent, idx[m.identity.as_str()]);
            let tid = &group_min[&root];
            if *tid != m.old_thread {
                self.conn
                    .execute(
                        "UPDATE messages SET thread_id = ? WHERE id = ?",
                        params![tid.clone(), m.id],
                    )
                    .await?;
                changed.push(m.id);
            }
        }
        Ok(changed)
    }

    // ---------- contacts ----------

    /// Update contact aggregates for one message's participants.
    pub async fn touch_contacts(&self, account_id: i64, n: &NormalizedMessage) -> Result<()> {
        for a in n.from.iter().chain(n.to.iter()).chain(n.cc.iter()) {
            let Some(email) = &a.email else { continue };
            if email.is_empty() || !email.contains('@') {
                continue;
            }
            let org = crate::normalize::org_for(email);
            let is_role = crate::normalize::is_role_address(email);
            let mut rows = self
                .conn
                .query(
                    "SELECT names_json, msg_count FROM contacts WHERE account_id = ? AND email = ?",
                    params![account_id, email.clone()],
                )
                .await?;
            let (mut names, count): (HashMap<String, i64>, i64) = match rows.next().await? {
                Some(r) => (serde_json::from_str(&r.get::<String>(0)?)?, r.get(1)?),
                None => (HashMap::new(), 0),
            };
            if let Some(name) = &a.name {
                *names.entry(name.clone()).or_insert(0) += 1;
            }
            self.conn
                .execute(
                    "INSERT INTO contacts (account_id, email, names_json, org, is_role, msg_count, last_seen)
                     VALUES (?,?,?,?,?,?,?)
                     ON CONFLICT(account_id, email) DO UPDATE SET
                       names_json = excluded.names_json, msg_count = excluded.msg_count,
                       last_seen = MAX(contacts.last_seen, excluded.last_seen)",
                    params![
                        account_id,
                        email.clone(),
                        serde_json::to_string(&names)?,
                        org,
                        is_role as i64,
                        count + 1,
                        n.date.canonical
                    ],
                )
                .await?;
        }
        Ok(())
    }

    pub async fn contacts(&self, account_id: i64, q: Option<&str>, limit: i64) -> Result<Vec<serde_json::Value>> {
        let like = format!("%{}%", q.unwrap_or(""));
        let mut rows = self
            .conn
            .query(
                "SELECT email, names_json, org, is_role, msg_count, last_seen FROM contacts
                 WHERE account_id = ? AND (email LIKE ? OR names_json LIKE ?)
                 ORDER BY msg_count DESC LIMIT ?",
                params![account_id, like.clone(), like, limit],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(r) = rows.next().await? {
            let names: HashMap<String, i64> = serde_json::from_str(&r.get::<String>(1)?)?;
            let display = names
                .iter()
                .max_by_key(|(_, c)| **c)
                .map(|(n, _)| n.clone());
            out.push(serde_json::json!({
                "email": r.get::<String>(0)?,
                "display_name": display,
                "names": names,
                "org": r.get::<Option<String>>(2)?,
                "is_role": r.get::<i64>(3)? != 0,
                "msg_count": r.get::<i64>(4)?,
                "last_seen": r.get::<i64>(5)?,
            }));
        }
        Ok(out)
    }

    pub async fn orgs(&self, account_id: i64, limit: i64) -> Result<Vec<serde_json::Value>> {
        let mut rows = self
            .conn
            .query(
                "SELECT org, COUNT(*) as contacts, SUM(msg_count) as msgs FROM contacts
                 WHERE account_id = ? AND org IS NOT NULL
                 GROUP BY org ORDER BY msgs DESC LIMIT ?",
                params![account_id, limit],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(r) = rows.next().await? {
            out.push(serde_json::json!({
                "org": r.get::<String>(0)?,
                "contact_count": r.get::<i64>(1)?,
                "msg_count": r.get::<i64>(2)?,
            }));
        }
        Ok(out)
    }

    // ---------- merges (append-only, survive rebuilds) ----------

    pub async fn add_merge_op(&self, account_id: i64, op: &str, a: &str, b: &str) -> Result<()> {
        let (a, b) = (a.to_ascii_lowercase(), b.to_ascii_lowercase());
        self.conn
            .execute(
                "INSERT INTO merge_ops (account_id, op, addr_a, addr_b, at) VALUES (?,?,?,?,?)",
                params![account_id, op, a, b, now()],
            )
            .await?;
        Ok(())
    }

    /// Current merge groups: replay the op log; merge adds an edge, unmerge
    /// removes one instance of it. Returns the connected component of `email`.
    pub async fn merge_group(&self, account_id: i64, email: &str) -> Result<Vec<String>> {
        let email = email.to_ascii_lowercase();
        let mut rows = self
            .conn
            .query(
                "SELECT op, addr_a, addr_b FROM merge_ops WHERE account_id = ? ORDER BY id",
                params![account_id],
            )
            .await?;
        let mut edges: Vec<(String, String)> = Vec::new();
        while let Some(r) = rows.next().await? {
            let op: String = r.get(0)?;
            let a: String = r.get(1)?;
            let b: String = r.get(2)?;
            if op == "merge" {
                edges.push((a, b));
            } else if let Some(pos) = edges
                .iter()
                .position(|(x, y)| (*x == a && *y == b) || (*x == b && *y == a))
            {
                edges.remove(pos);
            }
        }
        // BFS from email over remaining edges
        let mut group = vec![email.clone()];
        let mut frontier = vec![email];
        while let Some(cur) = frontier.pop() {
            for (a, b) in &edges {
                let other = if *a == cur {
                    b
                } else if *b == cur {
                    a
                } else {
                    continue;
                };
                if !group.contains(other) {
                    group.push(other.clone());
                    frontier.push(other.clone());
                }
            }
        }
        group.sort();
        Ok(group)
    }

    pub async fn merge_ops(&self, account_id: i64) -> Result<Vec<serde_json::Value>> {
        let mut rows = self
            .conn
            .query(
                "SELECT op, addr_a, addr_b, at FROM merge_ops WHERE account_id = ? ORDER BY id",
                params![account_id],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(r) = rows.next().await? {
            out.push(serde_json::json!({
                "op": r.get::<String>(0)?,
                "a": r.get::<String>(1)?,
                "b": r.get::<String>(2)?,
                "at": r.get::<i64>(3)?,
            }));
        }
        Ok(out)
    }

    // ---------- image allowances ----------

    pub async fn allow_images(&self, account_id: i64, sender: &str) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO image_allow (account_id, sender, at) VALUES (?,?,?)",
                params![account_id, sender.to_ascii_lowercase(), now()],
            )
            .await?;
        Ok(())
    }

    pub async fn image_allowances(&self, account_id: i64) -> Result<Vec<serde_json::Value>> {
        let mut rows = self
            .conn
            .query(
                "SELECT sender, at FROM image_allow WHERE account_id = ? ORDER BY at DESC",
                params![account_id],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(r) = rows.next().await? {
            out.push(serde_json::json!({
                "sender": r.get::<String>(0)?,
                "at": r.get::<i64>(1)?,
            }));
        }
        Ok(out)
    }

    pub async fn revoke_images(&self, account_id: i64, sender: &str) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM image_allow WHERE account_id = ? AND sender = ?",
                params![account_id, sender.to_ascii_lowercase()],
            )
            .await?;
        Ok(())
    }

    pub async fn rename_account(&self, id: i64, name: &str) -> Result<()> {
        self.conn
            .execute("UPDATE accounts SET name = ? WHERE id = ?", params![name, id])
            .await?;
        Ok(())
    }

    pub async fn images_allowed(&self, account_id: i64, sender: &str) -> Result<bool> {
        let mut rows = self
            .conn
            .query(
                "SELECT 1 FROM image_allow WHERE account_id = ? AND sender = ?",
                params![account_id, sender.to_ascii_lowercase()],
            )
            .await?;
        Ok(rows.next().await?.is_some())
    }

    // ---------- rebuild (layer 2 from layer 1) ----------

    /// All message ids for an account, for rebuild/reindex passes.
    pub async fn message_ids(&self, account_id: i64) -> Result<Vec<i64>> {
        let mut rows = self
            .conn
            .query("SELECT id FROM messages WHERE account_id = ?", params![account_id])
            .await?;
        let mut out = Vec::new();
        while let Some(r) = rows.next().await? {
            out.push(r.get(0)?);
        }
        Ok(out)
    }

    /// Reconstruct the layer-1 RawMessage for a stored message.
    pub async fn raw_message(&self, id: i64) -> Result<RawMessage> {
        let header_bytes = self.raw_headers(id).await?;
        let mut rows = self
            .conn
            .query(
                "SELECT path, kind, mime, filename, size, content_id, inline_flag, content
                 FROM parts WHERE message_id = ? ORDER BY id",
                params![id],
            )
            .await?;
        let mut text_parts = Vec::new();
        let mut attachments = Vec::new();
        while let Some(r) = rows.next().await? {
            let kind: String = r.get(1)?;
            if kind == "text" {
                text_parts.push(TextPartRaw {
                    path: r.get(0)?,
                    entity: r.get::<Option<Vec<u8>>>(7)?.unwrap_or_default(),
                });
            } else {
                attachments.push(AttachMeta {
                    path: r.get(0)?,
                    mime: r.get(2)?,
                    filename: r.get(3)?,
                    size: r.get::<i64>(4)? as u64,
                    content_id: r.get(5)?,
                    inline: r.get::<i64>(6)? != 0,
                });
            }
        }
        let mut rows = self
            .conn
            .query(
                "SELECT internaldate FROM locations WHERE message_id = ? ORDER BY internaldate LIMIT 1",
                params![id],
            )
            .await?;
        let internaldate = match rows.next().await? {
            Some(r) => r.get::<Option<i64>>(0)?,
            None => None,
        };
        let mut rows = self
            .conn
            .query("SELECT flags FROM messages WHERE id = ?", params![id])
            .await?;
        let flags: Vec<String> = match rows.next().await? {
            Some(r) => serde_json::from_str(&r.get::<String>(0)?)?,
            None => vec![],
        };
        Ok(RawMessage {
            header_bytes,
            text_parts,
            attachments,
            internaldate,
            flags,
        })
    }

    /// Overwrite the derived (layer 2) columns of one message from a fresh
    /// normalisation. Identity is layer 1 and does not change.
    pub async fn update_derived(&self, id: i64, n: &NormalizedMessage) -> Result<()> {
        self.conn
            .execute(
                "UPDATE messages SET subject = ?, subject_norm = ?, from_json = ?, to_json = ?,
                        cc_json = ?, date_canonical = ?, date_source = ?, date_offset_mins = ?,
                        received_top = ?, date_hdr = ?, internaldate = ?, skew = ?, refs_json = ?,
                        body_text = ?, spans_json = ?, has_html = ? WHERE id = ?",
                params![
                    n.subject.clone(),
                    n.subject_norm.clone(),
                    serde_json::to_string(&n.from)?,
                    serde_json::to_string(&n.to)?,
                    serde_json::to_string(&n.cc)?,
                    n.date.canonical,
                    n.date.source.as_str(),
                    n.date.offset_mins as i64,
                    n.date.received_top,
                    n.date.date_hdr,
                    n.date.internaldate,
                    n.date.skew as i64,
                    serde_json::to_string(&n.references)?,
                    n.body_text.clone(),
                    serde_json::to_string(&n.spans)?,
                    n.has_html as i64,
                    id
                ],
            )
            .await?;
        Ok(())
    }
}

fn row_to_message(r: &libsql::Row) -> Result<StoredMessage> {
    Ok(StoredMessage {
        id: r.get(0)?,
        account_id: r.get(1)?,
        identity: r.get(2)?,
        msgid: r.get(3)?,
        subject: r.get(4)?,
        subject_norm: r.get(5)?,
        from: serde_json::from_str(&r.get::<String>(6)?)?,
        to: serde_json::from_str(&r.get::<String>(7)?)?,
        cc: serde_json::from_str(&r.get::<String>(8)?)?,
        date_canonical: r.get(9)?,
        date_source: r.get(10)?,
        date_offset_mins: r.get::<i64>(11)? as i32,
        received_top: r.get(12)?,
        date_hdr: r.get(13)?,
        internaldate: r.get(14)?,
        skew: r.get::<i64>(15)? != 0,
        thread_id: r.get(16)?,
        references: serde_json::from_str(&r.get::<String>(17)?)?,
        body_text: r.get(18)?,
        spans: serde_json::from_str(&r.get::<String>(19)?)?,
        has_html: r.get::<i64>(20)? != 0,
        has_attach: r.get::<i64>(21)? != 0,
    })
}

impl StoredMessage {
    pub fn fresh_text(&self) -> String {
        self.spans
            .iter()
            .filter(|s| !s.quoted)
            .map(|s| self.body_text.get(s.start..s.end).unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n")
    }
    pub fn quoted_text(&self) -> String {
        self.spans
            .iter()
            .filter(|s| s.quoted)
            .map(|s| self.body_text.get(s.start..s.end).unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n")
    }
}
