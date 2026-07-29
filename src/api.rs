//! JSON HTTP API. One screen, everything is a query.

use crate::crypto::Crypto;
use crate::index::{Indexes, SearchOptions};
use crate::queryparse::{Clause, FilterField};
use crate::source::{FixtureSource, MailSource};
use crate::store::{Account, Store};
use crate::sync::ProgressMap;
use anyhow::{Context, Result};
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

pub struct App {
    pub store: Store,
    pub indexes: Indexes,
    pub crypto: Option<Crypto>,
    pub progress: ProgressMap,
}

pub type SharedApp = Arc<App>;

// ---------- error plumbing ----------

pub struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"))
    }
}

fn bad(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.into())
}

fn not_found(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::NOT_FOUND, msg.into())
}

type ApiResult<T> = Result<T, ApiError>;

// ---------- sources ----------

pub fn make_source(account: &Account, crypto: &Option<Crypto>) -> Result<Box<dyn MailSource>> {
    if account.kind == "fixture" {
        Ok(Box::new(FixtureSource::new(account.host.clone().into())))
    } else {
        let crypto = crypto
            .as_ref()
            .context("MAILGREP_KEY not configured; cannot decrypt IMAP credentials")?;
        let password = crypto.decrypt(&account.password_enc)?;
        Ok(Box::new(crate::imapsource::ImapSource::connect(
            &account.host,
            account.port,
            &account.username,
            &password,
        )?))
    }
}

// ---------- router ----------

pub fn router(app: SharedApp) -> Router {
    Router::new()
        .route("/api/status", get(status))
        .route("/api/accounts", get(list_accounts).post(add_account))
        .route("/api/accounts/{id}", delete(remove_account))
        .route("/api/accounts/{id}/folders", get(account_folders).put(set_folders))
        .route("/api/accounts/{id}/sync", post(trigger_sync))
        .route("/api/accounts/{id}/rebuild", post(rebuild))
        .route("/api/accounts/{id}/interval", post(set_interval))
        .route("/api/search", get(search))
        .route("/api/message/{id}", get(message_detail))
        .route("/api/message/{id}/html", get(message_html))
        .route("/api/message/{id}/raw", get(message_raw))
        .route("/api/message/{id}/part/{*path}", get(message_part))
        .route("/api/message/{id}/cid/{cid}", get(message_cid))
        .route("/api/message/{id}/allow_images", post(allow_images))
        .route("/api/thread/{account}/{*tid}", get(thread_view))
        .route("/api/contacts", get(contacts))
        .route("/api/orgs", get(orgs))
        .route("/api/merge", post(merge))
        .route("/api/unmerge", post(unmerge))
        .route("/api/merges", get(merges))
        .with_state(app)
}

// ---------- status ----------

async fn status(State(app): State<SharedApp>) -> ApiResult<Json<Value>> {
    let accounts = app.store.accounts().await?;
    let progress = app.progress.lock().unwrap().clone();
    let mut out = Vec::new();
    for a in &accounts {
        let count = app.store.message_count(a.id).await?;
        let logs = app.store.sync_logs(a.id, 5).await?;
        out.push(json!({
            "id": a.id,
            "name": a.name,
            "kind": a.kind,
            "host": a.host,
            "username": a.username,
            "message_count": count,
            "index_size_bytes": app.indexes.index_size_bytes(a.id),
            "last_sync_at": a.last_sync_at,
            "last_sync_status": a.last_sync_status,
            "sync_interval_mins": a.sync_interval_mins,
            "excluded_folders": a.excluded_folders,
            "progress": progress.get(&a.id),
            "recent_syncs": logs,
        }));
    }
    Ok(Json(json!({ "accounts": out, "version": env!("CARGO_PKG_VERSION") })))
}

// ---------- accounts ----------

async fn list_accounts(State(app): State<SharedApp>) -> ApiResult<Json<Value>> {
    let accounts = app.store.accounts().await?;
    Ok(Json(json!(accounts)))
}

#[derive(Deserialize)]
struct NewAccount {
    name: String,
    #[serde(default)]
    host: String,
    #[serde(default = "default_port")]
    port: u16,
    #[serde(default)]
    username: String,
    #[serde(default)]
    password: String,
    /// fixture dir instead of imap
    #[serde(default)]
    fixture_dir: Option<String>,
}

fn default_port() -> u16 {
    993
}

async fn add_account(
    State(app): State<SharedApp>,
    Json(body): Json<NewAccount>,
) -> ApiResult<Json<Value>> {
    if let Some(dir) = &body.fixture_dir {
        if !std::path::Path::new(dir).is_dir() {
            return Err(bad("fixture_dir does not exist"));
        }
        let id = app
            .store
            .add_account(&body.name, "fixture", dir, 0, "", &[])
            .await?;
        return Ok(Json(json!({ "id": id })));
    }
    if body.host.is_empty() || body.username.is_empty() || body.password.is_empty() {
        return Err(bad("host, username and password are required"));
    }
    // Verify credentials before storing anything (story 4).
    let (host, port, user, pass) = (
        body.host.clone(),
        body.port,
        body.username.clone(),
        body.password.clone(),
    );
    tokio::task::spawn_blocking(move || {
        crate::imapsource::ImapSource::connect(&host, port, &user, &pass).map(|_| ())
    })
    .await
    .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map_err(|e| bad(format!("IMAP connection failed: {e:#}")))?;

    let crypto = app
        .crypto
        .as_ref()
        .ok_or_else(|| bad("MAILGREP_KEY not set; refusing to store credentials"))?;
    let enc = crypto.encrypt(&body.password)?;
    let id = app
        .store
        .add_account(&body.name, "imap", &body.host, body.port, &body.username, &enc)
        .await?;
    Ok(Json(json!({ "id": id })))
}

async fn remove_account(
    State(app): State<SharedApp>,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    app.store.delete_account(id).await?;
    app.indexes.drop_account(id)?;
    Ok(Json(json!({ "deleted": id })))
}

async fn account_folders(
    State(app): State<SharedApp>,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    let account = app.store.account(id).await?;
    let excluded = account.excluded_folders.clone();
    let crypto_ref = &app.crypto;
    let mut source = make_source(&account, crypto_ref).map_err(|e| bad(format!("{e:#}")))?;
    let folders =
        tokio::task::spawn_blocking(move || -> Result<Vec<String>> { source.folders() })
            .await
            .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))??;
    let out: Vec<Value> = folders
        .iter()
        .map(|f| {
            json!({
                "name": f,
                "excluded": excluded.iter().any(|e| e.eq_ignore_ascii_case(f)),
            })
        })
        .collect();
    Ok(Json(json!({ "folders": out })))
}

#[derive(Deserialize)]
struct FolderExclusion {
    excluded: Vec<String>,
}

async fn set_folders(
    State(app): State<SharedApp>,
    Path(id): Path<i64>,
    Json(body): Json<FolderExclusion>,
) -> ApiResult<Json<Value>> {
    app.store.set_excluded_folders(id, &body.excluded).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct IntervalBody {
    minutes: i64,
}

async fn set_interval(
    State(app): State<SharedApp>,
    Path(id): Path<i64>,
    Json(body): Json<IntervalBody>,
) -> ApiResult<Json<Value>> {
    app.store.set_sync_interval(id, body.minutes.max(0)).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct SyncBody {
    #[serde(default)]
    max_per_folder: Option<usize>,
}

async fn trigger_sync(
    State(app): State<SharedApp>,
    Path(id): Path<i64>,
    body: Option<Json<SyncBody>>,
) -> ApiResult<Json<Value>> {
    let max = body.and_then(|b| b.max_per_folder);
    start_sync(app.clone(), id, max).await.map_err(|e| bad(format!("{e:#}")))?;
    Ok(Json(json!({ "started": true })))
}

/// Spawn a sync for an account unless one is already running.
pub async fn start_sync(app: SharedApp, account_id: i64, max_per_folder: Option<usize>) -> Result<()> {
    {
        let progress = app.progress.lock().unwrap();
        if progress.get(&account_id).map(|p| p.running).unwrap_or(false) {
            anyhow::bail!("sync already running");
        }
    }
    let account = app.store.account(account_id).await?;
    let mut source = make_source(&account, &app.crypto)?;
    let handle = tokio::runtime::Handle::current();
    let app2 = app.clone();
    // mark running immediately so double-triggers are rejected
    app.progress.lock().unwrap().insert(
        account_id,
        crate::sync::SyncProgress {
            running: true,
            ..Default::default()
        },
    );
    tokio::task::spawn_blocking(move || {
        let res = crate::sync::run_sync(
            handle.clone(),
            &app2.store,
            &app2.indexes,
            source.as_mut(),
            account_id,
            &account.excluded_folders,
            max_per_folder,
            &app2.progress,
        );
        if let Err(e) = &res {
            tracing::error!(account_id, "sync failed: {e:#}");
            let mut p = app2
                .progress
                .lock()
                .unwrap()
                .get(&account_id)
                .cloned()
                .unwrap_or_default();
            p.running = false;
            p.error = Some(format!("{e:#}"));
            app2.progress.lock().unwrap().insert(account_id, p);
            handle
                .block_on(app2.store.set_sync_result(account_id, &format!("error: {e:#}")))
                .ok();
        }
    });
    Ok(())
}

async fn rebuild(State(app): State<SharedApp>, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let count = crate::sync::rebuild_account(&app.store, &app.indexes, id).await?;
    Ok(Json(json!({ "rebuilt": count })))
}

// ---------- search ----------

#[derive(Deserialize)]
struct SearchParams {
    #[serde(default)]
    q: String,
    /// account id, or "all"
    #[serde(default)]
    account: Option<String>,
    #[serde(default)]
    sort: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    offset: Option<usize>,
}

async fn search(
    State(app): State<SharedApp>,
    Query(p): Query<SearchParams>,
) -> ApiResult<Json<Value>> {
    let ast = crate::queryparse::parse(&p.q)
        .map_err(|e| bad(format!("{e}")))?;

    // Expand contact: values through merge groups (query-time application of
    // user merges), per account.
    let accounts = app.store.accounts().await?;
    if accounts.is_empty() {
        return Ok(Json(json!({
            "results": [], "total": 0, "facets": empty_facets(), "accounts": []
        })));
    }
    let selected: Vec<&Account> = match p.account.as_deref() {
        None => vec![&accounts[0]],
        Some("all") => accounts.iter().collect(),
        Some(id) => {
            let id: i64 = id.parse().map_err(|_| bad("bad account id"))?;
            vec![accounts.iter().find(|a| a.id == id).ok_or_else(|| not_found("no such account"))?]
        }
    };

    let opts = SearchOptions {
        limit: p.limit.unwrap_or(50).min(200),
        offset: p.offset.unwrap_or(0),
        sort_by_date: p.sort.as_deref() == Some("date"),
        facet_cap: 2000,
    };

    let mut per_account = Vec::new();
    for account in &selected {
        let mut ast_x = ast.clone();
        for c in ast_x.clauses.iter_mut() {
            if let Clause::Filter { field: FilterField::Contact, values, .. } = c {
                let mut expanded = Vec::new();
                for v in values.iter() {
                    for e in app.store.merge_group(account.id, v).await? {
                        if !expanded.contains(&e) {
                            expanded.push(e);
                        }
                    }
                }
                *values = expanded;
            }
        }
        let index = app.indexes.get(account.id)?;
        let out = index.search(&ast_x, &opts).map_err(ApiError::from)?;
        per_account.push((account.id, account.name.clone(), out));
    }

    // Single account: straight page. Multi: interleave separately-ranked
    // pages; no unified relevance across accounts (and labelled as such).
    let cross_account = per_account.len() > 1;
    let mut rows: Vec<(i64, String, crate::index::Hit)> = Vec::new();
    if cross_account {
        let mut iters: Vec<_> = per_account
            .iter()
            .map(|(id, name, out)| (id, name, out.page.iter()))
            .collect();
        loop {
            let mut any = false;
            for (id, name, it) in iters.iter_mut() {
                if let Some(hit) = it.next() {
                    rows.push((
                        **id,
                        (*name).clone(),
                        crate::index::Hit {
                            db_id: hit.db_id,
                            score: hit.score,
                            snippet: hit.snippet.clone(),
                        },
                    ));
                    any = true;
                }
            }
            if !any {
                break;
            }
        }
    } else {
        let (id, name, out) = &per_account[0];
        for hit in &out.page {
            rows.push((
                *id,
                name.clone(),
                crate::index::Hit {
                    db_id: hit.db_id,
                    score: hit.score,
                    snippet: hit.snippet.clone(),
                },
            ));
        }
    }

    // Hydrate page rows from the store.
    let ids: Vec<i64> = rows.iter().map(|(_, _, h)| h.db_id).collect();
    let msgs = app.store.messages_by_ids(&ids).await?;
    let msg_by_id: HashMap<i64, &crate::store::StoredMessage> =
        msgs.iter().map(|m| (m.id, m)).collect();
    let mut thread_ids: Vec<String> = Vec::new();
    for m in &msgs {
        if !m.thread_id.is_empty() && !thread_ids.contains(&m.thread_id) {
            thread_ids.push(m.thread_id.clone());
        }
    }
    let mut thread_sizes: HashMap<(i64, String), i64> = HashMap::new();
    for (aid, _, _) in &rows {
        let sizes = app
            .store
            .thread_sizes(*aid, &thread_ids)
            .await?;
        for (tid, n) in sizes {
            thread_sizes.insert((*aid, tid), n);
        }
    }

    let mut results = Vec::new();
    for (aid, aname, hit) in &rows {
        let Some(m) = msg_by_id.get(&hit.db_id) else { continue };
        let locs = app.store.locations(m.id).await?;
        results.push(json!({
            "id": m.id,
            "account_id": aid,
            "account": aname,
            "subject": if m.subject_norm.is_empty() { m.subject.clone() } else { m.subject_norm.clone() },
            "subject_raw": m.subject,
            "from": m.from,
            "date": m.date_canonical,
            "date_offset_mins": m.date_offset_mins,
            "date_source": m.date_source,
            "skew": m.skew,
            "has_attach": m.has_attach,
            "thread_id": m.thread_id,
            "thread_size": thread_sizes.get(&(*aid, m.thread_id.clone())).copied().unwrap_or(1),
            "folders": locs.iter().map(|l| l.folder.clone()).collect::<Vec<_>>(),
            "snippet": hit.snippet,
            "score": hit.score,
        }));
    }

    // Facets over the top hits of every selected account.
    let mut facet_ids: Vec<i64> = Vec::new();
    for (_, _, out) in &per_account {
        facet_ids.extend(&out.facet_ids);
    }
    let facets = compute_facets(&app, &facet_ids).await?;

    let total: usize = per_account.iter().map(|(_, _, o)| o.total).sum();
    Ok(Json(json!({
        "results": results,
        "total": total,
        "cross_account": cross_account,
        "facets": facets,
        "accounts": selected.iter().map(|a| json!({"id": a.id, "name": a.name})).collect::<Vec<_>>(),
    })))
}

fn empty_facets() -> Value {
    json!({ "senders": [], "orgs": [], "years": [], "exts": [], "folders": [] })
}

/// Facets computed over the (capped) top hits — top senders, top orgs, year
/// histogram, attachment types, folders.
/// ponytail: facet base is the top 2000 hits, not the full match set; switch
/// to fast-field collection if that approximation ever misleads.
async fn compute_facets(app: &SharedApp, ids: &[i64]) -> Result<Value> {
    let msgs = app.store.messages_by_ids(ids).await?;
    let mut senders: HashMap<String, (Option<String>, i64)> = HashMap::new();
    let mut orgs: HashMap<String, i64> = HashMap::new();
    let mut years: HashMap<i32, i64> = HashMap::new();
    for m in &msgs {
        if let Some(a) = m.from.first() {
            if let Some(e) = &a.email {
                let entry = senders.entry(e.clone()).or_insert((a.name.clone(), 0));
                entry.1 += 1;
                if entry.0.is_none() {
                    entry.0 = a.name.clone();
                }
                if let Some(org) = crate::normalize::org_for(e) {
                    *orgs.entry(org).or_insert(0) += 1;
                }
            }
        }
        let year = chrono::DateTime::from_timestamp(m.date_canonical, 0)
            .map(|d| chrono::Datelike::year(&d))
            .unwrap_or(1970);
        *years.entry(year).or_insert(0) += 1;
    }
    // attachment extensions for messages that have any
    let with_attach: Vec<i64> = msgs.iter().filter(|m| m.has_attach).map(|m| m.id).collect();
    let mut exts: HashMap<String, i64> = HashMap::new();
    for chunk in with_attach.chunks(500) {
        for id in chunk {
            for p in app.store.parts(*id).await? {
                if p.kind == "attach" {
                    let meta = crate::types::AttachMeta {
                        path: p.path,
                        filename: p.filename,
                        mime: p.mime,
                        size: p.size,
                        content_id: p.content_id,
                        inline: p.inline,
                    };
                    if let Some(e) = meta.ext() {
                        *exts.entry(e).or_insert(0) += 1;
                    } else {
                        *exts.entry(meta.mime_category()).or_insert(0) += 1;
                    }
                }
            }
        }
    }

    let mut senders: Vec<Value> = senders
        .into_iter()
        .map(|(email, (name, count))| json!({ "email": email, "name": name, "count": count }))
        .collect();
    senders.sort_by_key(|v| -v["count"].as_i64().unwrap_or(0));
    senders.truncate(12);
    let mut orgs: Vec<Value> = orgs
        .into_iter()
        .map(|(org, count)| json!({ "org": org, "count": count }))
        .collect();
    orgs.sort_by_key(|v| -v["count"].as_i64().unwrap_or(0));
    orgs.truncate(12);
    let mut years: Vec<Value> = years
        .into_iter()
        .map(|(y, c)| json!({ "year": y, "count": c }))
        .collect();
    years.sort_by_key(|v| v["year"].as_i64().unwrap_or(0));
    let mut exts: Vec<Value> = exts
        .into_iter()
        .map(|(e, c)| json!({ "ext": e, "count": c }))
        .collect();
    exts.sort_by_key(|v| -v["count"].as_i64().unwrap_or(0));
    exts.truncate(12);

    Ok(json!({ "senders": senders, "orgs": orgs, "years": years, "exts": exts }))
}

// ---------- message views ----------

async fn message_detail(
    State(app): State<SharedApp>,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    let m = app.store.message(id).await?.ok_or_else(|| not_found("no such message"))?;
    let parts = app.store.parts(id).await?;
    let locs = app.store.locations(id).await?;
    let thread = app.store.thread_messages(m.account_id, &m.thread_id).await?;
    let sender = m.from.first().and_then(|a| a.email.clone()).unwrap_or_default();
    let images_allowed = app.store.images_allowed(m.account_id, &sender).await?;
    let attachments: Vec<Value> = parts
        .iter()
        .filter(|p| p.kind == "attach")
        .map(|p| {
            json!({
                "path": p.path,
                "filename": p.filename,
                "mime": p.mime,
                "size": p.size,
                "content_id": p.content_id,
                "inline": p.inline,
            })
        })
        .collect();
    Ok(Json(json!({
        "id": m.id,
        "account_id": m.account_id,
        "identity": m.identity,
        "msgid": m.msgid,
        "subject": m.subject,
        "subject_norm": m.subject_norm,
        "from": m.from,
        "to": m.to,
        "cc": m.cc,
        "dates": {
            "canonical": m.date_canonical,
            "source": m.date_source,
            "offset_mins": m.date_offset_mins,
            "received_top": m.received_top,
            "date_header": m.date_hdr,
            "internaldate": m.internaldate,
            "skew": m.skew,
        },
        "thread_id": m.thread_id,
        "thread_size": thread.len(),
        "references": m.references,
        "body_text": m.body_text,
        "fresh_text": m.fresh_text(),
        "has_html": m.has_html,
        "attachments": attachments,
        "folders": locs.iter().map(|l| json!({"folder": l.folder, "uid": l.uid})).collect::<Vec<_>>(),
        "images_allowed": images_allowed,
    })))
}

#[derive(Deserialize)]
struct HtmlParams {
    #[serde(default)]
    images: Option<u8>,
}

async fn message_html(
    State(app): State<SharedApp>,
    Path(id): Path<i64>,
    Query(p): Query<HtmlParams>,
) -> ApiResult<Response> {
    let m = app.store.message(id).await?.ok_or_else(|| not_found("no such message"))?;
    let parts = app.store.parts(id).await?;
    let sender = m.from.first().and_then(|a| a.email.clone()).unwrap_or_default();
    let allowed = p.images == Some(1) || app.store.images_allowed(m.account_id, &sender).await?;

    // choose html text part if present, else render plain text preformatted
    let html_part = parts.iter().find(|p| p.kind == "text" && p.mime.contains("html"));
    let body_html = match html_part {
        Some(part) => {
            let entity = app
                .store
                .text_part_content(id, &part.path)
                .await?
                .unwrap_or_default();
            mail_parser::MessageParser::default()
                .parse(&entity)
                .and_then(|pm| pm.body_html(0).map(|c| c.to_string()))
                .unwrap_or_default()
        }
        None => {
            let text = askama_escape(&m.body_text);
            format!("<pre style=\"white-space:pre-wrap;font:14px/1.5 system-ui\">{text}</pre>")
        }
    };

    // cid -> part path map for inline images
    let cid_map: HashMap<String, String> = parts
        .iter()
        .filter(|p| p.kind == "attach")
        .filter_map(|p| p.content_id.clone().map(|c| (c, p.path.clone())))
        .collect();

    let sanitized = sanitize_html(&body_html, id, &cid_map, allowed);
    let csp = if allowed {
        "default-src 'none'; img-src 'self' https: http: data: cid:; style-src 'unsafe-inline'"
    } else {
        "default-src 'none'; img-src 'self' data:; style-src 'unsafe-inline'"
    };
    Ok((
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CONTENT_SECURITY_POLICY, csp),
            (header::X_FRAME_OPTIONS, "SAMEORIGIN"),
        ],
        sanitized,
    )
        .into_response())
}

fn askama_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn sanitize_html(html: &str, msg_id: i64, cid_map: &HashMap<String, String>, allow_remote: bool) -> String {
    use ammonia::Builder;
    let mut b = Builder::default();
    b.add_tags(["img"])
        .add_tag_attributes("img", ["src", "alt", "width", "height"])
        .add_generic_attributes(["style"])
        .rm_tags(["form", "input", "button", "select", "textarea"])
        .url_schemes(["http", "https", "mailto", "cid"].into_iter().collect());
    let cid_map = cid_map.clone();
    b.attribute_filter(move |element, attribute, value| {
        if element == "img" && attribute == "src" {
            if let Some(cid) = value.strip_prefix("cid:") {
                let cid = cid.trim_matches(['<', '>']);
                if let Some(path) = cid_map.get(cid) {
                    return Some(format!("/api/message/{msg_id}/part/{path}").into());
                }
                return None;
            }
            if value.starts_with("data:") {
                return Some(value.into());
            }
            if !allow_remote {
                // tracking pixels stay dark by default
                return None;
            }
        }
        Some(value.into())
    });
    b.clean(html).to_string()
}

async fn message_raw(State(app): State<SharedApp>, Path(id): Path<i64>) -> ApiResult<Response> {
    let headers = app.store.raw_headers(id).await?;
    let parts = app.store.parts(id).await?;
    let mut out = headers;
    for p in parts.iter().filter(|p| p.kind == "text") {
        if let Some(content) = app.store.text_part_content(id, &p.path).await? {
            out.extend_from_slice(format!("\r\n--- text part {} ---\r\n", p.path).as_bytes());
            out.extend_from_slice(&content);
        }
    }
    for p in parts.iter().filter(|p| p.kind == "attach") {
        out.extend_from_slice(
            format!(
                "\r\n--- attachment part {} ({}, {} bytes{}) — bytes not stored, fetched on demand ---\r\n",
                p.path,
                p.mime,
                p.size,
                p.filename.as_deref().map(|f| format!(", {f}")).unwrap_or_default()
            )
            .as_bytes(),
        );
    }
    Ok((
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        out,
    )
        .into_response())
}

/// Attachment / part bytes, fetched live from the source. Verifies the
/// stored location still resolves; relocates by Message-ID when it does not.
async fn message_part(
    State(app): State<SharedApp>,
    Path((id, path)): Path<(i64, String)>,
) -> ApiResult<Response> {
    let m = app.store.message(id).await?.ok_or_else(|| not_found("no such message"))?;
    let parts = app.store.parts(id).await?;
    let part = parts
        .iter()
        .find(|p| p.path == path)
        .ok_or_else(|| not_found("no such part"))?;

    // text parts are stored locally
    if part.kind == "text" {
        if let Some(content) = app.store.text_part_content(id, &path).await? {
            return Ok(([(header::CONTENT_TYPE, part.mime.clone())], content).into_response());
        }
    }

    let account = app.store.account(m.account_id).await?;
    let locs = app.store.locations(id).await?;
    let msgid = m.msgid.clone();
    let mime = part.mime.clone();
    let filename = part.filename.clone().unwrap_or_else(|| format!("part-{path}"));
    let app2 = app.clone();
    let crypto_ref = &app.crypto;
    let mut source = make_source(&account, crypto_ref).map_err(|e| bad(format!("{e:#}")))?;
    let path2 = path.clone();

    let fetch = tokio::task::spawn_blocking(move || -> Result<(Vec<u8>, Option<(String, u32, u32)>)> {
        // last-known locations first
        for loc in &locs {
            if let Ok(bytes) = source.fetch_part(&loc.folder, loc.uid, &path2) {
                if !bytes.is_empty() {
                    return Ok((bytes, None));
                }
            }
        }
        // relocate via Message-ID search
        if let Some(msgid) = &msgid {
            if let Some((folder, uid, uidvalidity)) = source.locate(msgid)? {
                let bytes = source.fetch_part(&folder, uid, &path2)?;
                return Ok((bytes, Some((folder, uid, uidvalidity))));
            }
        }
        anyhow::bail!(
            "message no longer on the server at any known location — check your mail client"
        )
    })
    .await
    .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let (bytes, relocated) = fetch.map_err(|e| ApiError(StatusCode::BAD_GATEWAY, format!("{e:#}")))?;
    if let Some((folder, uid, uidvalidity)) = relocated {
        app2.store
            .update_location(
                id,
                m.account_id,
                &crate::store::StoredLocation {
                    folder,
                    uid,
                    uidvalidity,
                    internaldate: None,
                },
            )
            .await?;
    }
    Ok((
        [
            (header::CONTENT_TYPE, mime),
            (
                header::CONTENT_DISPOSITION,
                format!("inline; filename=\"{}\"", filename.replace('"', "")),
            ),
        ],
        bytes,
    )
        .into_response())
}

async fn message_cid(
    State(app): State<SharedApp>,
    Path((id, cid)): Path<(i64, String)>,
) -> ApiResult<Response> {
    let parts = app.store.parts(id).await?;
    let part = parts
        .iter()
        .find(|p| p.content_id.as_deref() == Some(cid.as_str()))
        .ok_or_else(|| not_found("no such cid"))?;
    let path = part.path.clone();
    message_part(State(app), Path((id, path))).await
}

async fn allow_images(
    State(app): State<SharedApp>,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    let m = app.store.message(id).await?.ok_or_else(|| not_found("no such message"))?;
    let sender = m
        .from
        .first()
        .and_then(|a| a.email.clone())
        .ok_or_else(|| bad("message has no sender"))?;
    app.store.allow_images(m.account_id, &sender).await?;
    Ok(Json(json!({ "allowed": sender })))
}

async fn thread_view(
    State(app): State<SharedApp>,
    Path((account, tid)): Path<(i64, String)>,
) -> ApiResult<Json<Value>> {
    let msgs = app.store.thread_messages(account, &tid).await?;
    let out: Vec<Value> = msgs
        .iter()
        .map(|m| {
            json!({
                "id": m.id,
                "subject": m.subject,
                "from": m.from,
                "date": m.date_canonical,
                "date_source": m.date_source,
                "skew": m.skew,
                "snippet": m.fresh_text().chars().take(200).collect::<String>(),
            })
        })
        .collect();
    let subject = msgs.first().map(|m| m.subject_norm.clone()).unwrap_or_default();
    Ok(Json(json!({ "thread_id": tid, "subject": subject, "messages": out })))
}

// ---------- contacts / orgs / merges ----------

#[derive(Deserialize)]
struct ContactParams {
    account: i64,
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    limit: Option<i64>,
}

async fn contacts(
    State(app): State<SharedApp>,
    Query(p): Query<ContactParams>,
) -> ApiResult<Json<Value>> {
    let list = app
        .store
        .contacts(p.account, p.q.as_deref(), p.limit.unwrap_or(100))
        .await?;
    Ok(Json(json!({ "contacts": list })))
}

#[derive(Deserialize)]
struct OrgParams {
    account: i64,
    #[serde(default)]
    limit: Option<i64>,
}

async fn orgs(State(app): State<SharedApp>, Query(p): Query<OrgParams>) -> ApiResult<Json<Value>> {
    let list = app.store.orgs(p.account, p.limit.unwrap_or(100)).await?;
    Ok(Json(json!({ "orgs": list })))
}

#[derive(Deserialize)]
struct MergeBody {
    account: i64,
    a: String,
    b: String,
}

async fn merge(State(app): State<SharedApp>, Json(b): Json<MergeBody>) -> ApiResult<Json<Value>> {
    app.store.add_merge_op(b.account, "merge", &b.a, &b.b).await?;
    let group = app.store.merge_group(b.account, &b.a).await?;
    Ok(Json(json!({ "group": group })))
}

async fn unmerge(State(app): State<SharedApp>, Json(b): Json<MergeBody>) -> ApiResult<Json<Value>> {
    app.store.add_merge_op(b.account, "unmerge", &b.a, &b.b).await?;
    let group = app.store.merge_group(b.account, &b.a).await?;
    Ok(Json(json!({ "group": group })))
}

#[derive(Deserialize)]
struct MergesParams {
    account: i64,
}

async fn merges(State(app): State<SharedApp>, Query(p): Query<MergesParams>) -> ApiResult<Json<Value>> {
    let ops = app.store.merge_ops(p.account).await?;
    Ok(Json(json!({ "ops": ops })))
}
