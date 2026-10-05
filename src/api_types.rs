//! Shared JSON API contracts exported to TypeScript by ts-rs.

use crate::store::Account;
use crate::types::Addr;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ErrorResponse {
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AuthStatus {
    pub required: bool,
    pub authenticated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LoginBody {
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StatusResponse {
    pub accounts: Vec<StatusAccount>,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StatusAccount {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub host: String,
    pub username: String,
    pub message_count: i64,
    pub index_size_bytes: u64,
    pub last_sync_at: Option<i64>,
    pub last_sync_status: Option<String>,
    pub sync_interval_mins: i64,
    pub excluded_folders: Vec<String>,
    pub progress: Option<SyncProgress>,
    pub recent_syncs: Vec<SyncLog>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SyncProgress {
    pub running: bool,
    pub folder: String,
    pub discovered: u64,
    pub processed: u64,
    pub new_msgs: u64,
    pub removed: u64,
    pub failed: u64,
    pub error: Option<String>,
}

impl From<crate::sync::SyncProgress> for SyncProgress {
    fn from(value: crate::sync::SyncProgress) -> Self {
        Self {
            running: value.running,
            folder: value.folder,
            discovered: value.discovered,
            processed: value.processed,
            new_msgs: value.new_msgs,
            removed: value.removed,
            failed: value.failed,
            error: value.error,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SyncLog {
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub new: i64,
    pub removed: i64,
    pub failed: i64,
    pub status: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AccountResponse {
    pub id: i64,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub excluded_folders: Vec<String>,
    pub sync_interval_mins: i64,
    pub last_sync_at: Option<i64>,
    pub last_sync_status: Option<String>,
    pub kind: String,
    pub security: String,
}

impl From<Account> for AccountResponse {
    fn from(value: Account) -> Self {
        Self {
            id: value.id,
            name: value.name,
            host: value.host,
            port: value.port,
            username: value.username,
            excluded_folders: value.excluded_folders,
            sync_interval_mins: value.sync_interval_mins,
            last_sync_at: value.last_sync_at,
            last_sync_status: value.last_sync_status,
            kind: value.kind,
            security: value.security,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct IdResponse {
    pub id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DeletedResponse {
    pub deleted: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FoldersResponse {
    pub folders: Vec<FolderInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FolderInfo {
    pub name: String,
    pub excluded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct OkResponse {
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SyncStartedResponse {
    pub started: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RebuildResponse {
    pub rebuilt: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ImageAllowancesResponse {
    pub allowances: Vec<ImageAllowance>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ImageAllowance {
    pub sender: String,
    pub at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SearchResponse {
    pub results: Vec<SearchRow>,
    pub total: usize,
    pub cross_account: bool,
    pub facets: FacetData,
    pub accounts: Vec<SearchAccount>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SearchRow {
    pub id: i64,
    pub account_id: i64,
    pub account: String,
    pub subject: String,
    pub subject_raw: String,
    pub r#from: Vec<Addr>,
    pub date: i64,
    pub date_offset_mins: i32,
    pub date_source: String,
    pub skew: bool,
    pub has_attach: bool,
    pub thread_id: String,
    pub thread_size: i64,
    pub folders: Vec<String>,
    pub snippet: String,
    pub score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SearchAccount {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FacetData {
    pub senders: Vec<SenderFacet>,
    pub orgs: Vec<OrgFacet>,
    pub years: Vec<YearFacet>,
    pub exts: Vec<ExtFacet>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SenderFacet {
    pub email: String,
    pub name: Option<String>,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct OrgFacet {
    pub org: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct YearFacet {
    pub year: i32,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ExtFacet {
    pub ext: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MessageDetail {
    pub id: i64,
    pub account_id: i64,
    pub identity: String,
    pub msgid: Option<String>,
    pub subject: String,
    pub subject_norm: String,
    pub r#from: Vec<Addr>,
    pub to: Vec<Addr>,
    pub cc: Vec<Addr>,
    pub dates: MessageDates,
    pub thread_id: String,
    pub thread_size: usize,
    pub references: Vec<String>,
    pub body_text: String,
    pub fresh_text: String,
    pub has_html: bool,
    pub attachments: Vec<Attachment>,
    pub folders: Vec<MessageFolder>,
    pub images_allowed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MessageDates {
    pub canonical: i64,
    pub source: String,
    pub offset_mins: i32,
    pub received_top: Option<i64>,
    pub date_header: Option<i64>,
    pub internaldate: Option<i64>,
    pub skew: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Attachment {
    pub path: String,
    pub filename: Option<String>,
    pub mime: String,
    pub size: u64,
    pub content_id: Option<String>,
    pub inline: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MessageFolder {
    pub folder: String,
    pub uid: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ImagesAllowedResponse {
    pub allowed: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ThreadResponse {
    pub thread_id: String,
    pub subject: String,
    pub messages: Vec<ThreadMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ThreadMessage {
    pub id: i64,
    pub subject: String,
    pub r#from: Vec<Addr>,
    pub date: i64,
    pub date_source: String,
    pub skew: bool,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ContactsResponse {
    pub contacts: Vec<Contact>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Contact {
    pub email: String,
    pub display_name: Option<String>,
    pub names: HashMap<String, i64>,
    pub org: Option<String>,
    pub is_role: bool,
    pub msg_count: i64,
    pub last_seen: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct OrgsResponse {
    pub orgs: Vec<OrgSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct OrgSummary {
    pub org: String,
    pub contact_count: i64,
    pub msg_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MergeGroupResponse {
    pub group: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MergesResponse {
    pub ops: Vec<MergeOperation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MergeOperation {
    pub op: String,
    pub a: String,
    pub b: String,
    pub at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NewAccount {
    pub name: String,
    #[serde(default)]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default = "default_security")]
    pub security: String,
    #[serde(default)]
    pub fixture_dir: Option<String>,
}

fn default_port() -> u16 {
    993
}

fn default_security() -> String {
    "ssl".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FolderExclusion {
    pub excluded: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct IntervalBody {
    pub minutes: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RenameBody {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RevokeBody {
    pub sender: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SyncBody {
    #[serde(default)]
    pub max_per_folder: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SearchParams {
    #[serde(default)]
    pub q: String,
    #[serde(default)]
    pub account: Option<String>,
    #[serde(default)]
    pub sort: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub offset: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ContactParams {
    pub account: i64,
    #[serde(default)]
    pub q: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct OrgParams {
    pub account: i64,
    #[serde(default)]
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MergeBody {
    pub account: i64,
    pub a: String,
    pub b: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MergesParams {
    pub account: i64,
}
