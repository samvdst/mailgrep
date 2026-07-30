//! Seam 1: the HTTP API over a fixture-backed mail source. A full app
//! instance ingests the committed corpus; everything is asserted through
//! the API, so internals are free to change.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use mailgrep::api::{self, App, SharedApp};
use mailgrep::index::Indexes;
use mailgrep::store::Store;
use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use tower::ServiceExt;

struct TestApp {
    router: axum::Router,
    app: SharedApp,
    corpus: PathBuf,
    account_id: i64,
    _tmp: tempfile::TempDir,
}

async fn req(router: &axum::Router, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let builder = Request::builder().method(method).uri(uri);
    let request = match body {
        Some(v) => builder
            .header("content-type", "application/json")
            .body(Body::from(v.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
    let resp = router.clone().oneshot(request).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

async fn raw_req(router: &axum::Router, uri: &str) -> (StatusCode, Vec<u8>, axum::http::HeaderMap) {
    let request = Request::builder().uri(uri).body(Body::empty()).unwrap();
    let resp = router.clone().oneshot(request).await.unwrap();
    let status = resp.status();
    let headers = resp.headers().clone();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes().to_vec();
    (status, bytes, headers)
}

fn search_uri(q: &str, account: i64) -> String {
    format!(
        "/api/search?account={account}&q={}",
        urlencode(q)
    )
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

async fn sync_account(app: &SharedApp, account_id: i64, max: Option<usize>) {
    let account = app.store.account(account_id).await.unwrap();
    let mut source = api::make_source(&account, &app.crypto).unwrap();
    let handle = tokio::runtime::Handle::current();
    let app2 = app.clone();
    tokio::task::spawn_blocking(move || {
        mailgrep::sync::run_sync(
            handle,
            &app2.store,
            &app2.indexes,
            source.as_mut(),
            account_id,
            &account.excluded_folders,
            max,
            &app2.progress,
        )
        .unwrap();
    })
    .await
    .unwrap();
}

/// Copy the committed corpus into a temp dir (tests mutate it) and pin file
/// mtimes to the message dates so INTERNALDATE-vs-canonical skew is
/// deterministic: only Archives/001-dup.eml keeps "now" as mtime.
async fn setup() -> TestApp {
    let tmp = tempfile::tempdir().unwrap();
    let corpus = tmp.path().join("corpus");
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/corpus");
    Command::new("cp").arg("-r").arg(&src).arg(&corpus).status().unwrap();

    let touches = [
        ("INBOX/001-boiler-root.eml", "2021-03-10 09:15:00"),
        ("INBOX/002-boiler-reply.eml", "2021-03-11 18:02:00"),
        ("INBOX/003-boiler-reply2.eml", "2021-03-12 10:30:00"),
        ("INBOX/004-no-msgid.eml", "2021-06-07 08:00:00"),
        ("INBOX/005-broken-date.eml", "2024-05-01 12:00:00"),
        ("INBOX/006-newsletter-cid.eml", "2021-09-04 12:00:00"),
        ("INBOX/007-outlook-fwd.eml", "2021-10-05 14:00:00"),
        ("INBOX/008-no-text.eml", "2022-02-02 09:00:00"),
        ("Sent/001-dup.eml", "2021-02-14 16:00:00"),
        ("Archives/002-photos.eml", "2021-08-08 20:00:00"),
        // Archives/001-dup.eml deliberately left at "now" -> skew
    ];
    for (file, date) in touches {
        Command::new("touch")
            .arg("-d")
            .arg(date)
            .arg(corpus.join(file))
            .status()
            .unwrap();
    }

    let data = tmp.path().join("data");
    std::fs::create_dir_all(&data).unwrap();
    let store = Store::open(data.join("db.sqlite").to_str().unwrap()).await.unwrap();
    let indexes = Indexes::new(data.join("index"));
    let app: SharedApp = Arc::new(App {
        store,
        indexes,
        crypto: None,
        progress: Default::default(),
    });
    let router = api::router(app.clone());

    let (status, body) = req(
        &router,
        "POST",
        "/api/accounts",
        Some(serde_json::json!({
            "name": "private",
            "fixture_dir": corpus.to_str().unwrap(),
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let account_id = body["id"].as_i64().unwrap();
    sync_account(&app, account_id, None).await;

    TestApp {
        router,
        app,
        corpus,
        account_id,
        _tmp: tmp,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn full_corpus_behaviour() {
    let t = setup().await;
    let r = &t.router;
    let aid = t.account_id;

    // ---- status: 11 files, 1 duplicate -> 10 messages
    let (s, body) = req(r, "GET", "/api/status", None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["accounts"][0]["message_count"], 10, "{body}");

    // ---- free text search + threading
    let (s, body) = req(r, "GET", &search_uri("boiler", aid), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["total"], 3, "{body}");
    let tid = body["results"][0]["thread_id"].as_str().unwrap().to_string();
    for res in body["results"].as_array().unwrap() {
        assert_eq!(res["thread_id"], tid.as_str());
        assert_eq!(res["thread_size"], 3);
        // prefix soup stripped for display
        assert_eq!(res["subject"], "Rechnung Boiler");
    }

    // ---- fresh beats quoted: "Klempner" is fresh in the root, quoted in the reply
    let (_, body) = req(r, "GET", &search_uri("klempner", aid), None).await;
    assert!(body["total"].as_i64().unwrap() >= 2, "{body}");
    assert_eq!(body["results"][0]["date_source"], "received");
    let first_from = body["results"][0]["from"][0]["email"].as_str().unwrap();
    assert_eq!(first_from, "hans@immo.ch", "fresh mention must outrank quoted: {body}");

    // ---- fuzzy matches a typo; quoted phrases never fuzz
    let (_, body) = req(r, "GET", &search_uri("klempmer", aid), None).await;
    assert!(body["total"].as_i64().unwrap() >= 1, "fuzzy failed: {body}");
    let (_, body) = req(r, "GET", &search_uri("\"zahle diese Woche\"", aid), None).await;
    assert!(body["total"].as_i64().unwrap() >= 1, "{body}");
    let (_, body) = req(r, "GET", &search_uri("\"zahle diese Wochen\"", aid), None).await;
    assert_eq!(body["total"], 0, "phrases must be exact: {body}");

    // ---- negation
    let (_, body) = req(r, "GET", &search_uri("boiler -klempner", aid), None).await;
    assert_eq!(body["total"], 1, "{body}");

    // ---- filters
    for (q, expect) in [
        ("from:@immo.ch", 2),
        ("from:hans@immo.ch", 2),
        ("from:immo.ch", 2),
        ("to:sam@demo.example boiler", 2), // two of the three boiler mails are to sam
        ("has:attachment", 4),
        ("ext:pdf", 2),
        ("ext:pdf,jpg", 3),
        ("attachment:image", 2),
        ("attachment:pdf", 1),
        ("filename:rechnung", 1),
        ("folder:Sent", 1),
        ("folder:archives", 2),
        ("date:2021", 8),
        ("before:2021-04", 4),
        ("after:2022", 2),
        ("dateskew:true", 1),
        // org matches mail to AND from the organisation, like contact:
        ("org:immo.ch", 3),
    ] {
        let (s, body) = req(r, "GET", &search_uri(q, aid), None).await;
        assert_eq!(s, StatusCode::OK, "query {q}: {body}");
        assert_eq!(body["total"], expect, "query `{q}` got {body}");
    }

    // ---- facets reflect the result set
    let (_, body) = req(r, "GET", &search_uri("", aid), None).await;
    let facets = &body["facets"];
    assert!(facets["senders"].as_array().unwrap().len() >= 3);
    assert!(facets["years"].as_array().unwrap().iter().any(|y| y["year"] == 2021));
    assert!(facets["exts"].as_array().unwrap().iter().any(|e| e["ext"] == "pdf"));
    assert!(facets["orgs"].as_array().unwrap().iter().any(|o| o["org"] == "immo.ch"));
    // freemail never becomes an organisation
    assert!(!facets["orgs"].as_array().unwrap().iter().any(|o| o["org"] == "gmail.com"));

    // ---- malformed query names the offending token
    let (s, body) = req(r, "GET", &search_uri("after:sometime", aid), None).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("after:sometime"), "{body}");

    // ---- dedupe: the message in Sent and Archives is one message, two folders
    let (_, body) = req(r, "GET", &search_uri("steuererklaerung", aid), None).await;
    assert_eq!(body["total"], 1, "{body}");
    let dup = &body["results"][0];
    let mut folders: Vec<String> = dup["folders"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_str().unwrap().to_string())
        .collect();
    folders.sort();
    assert_eq!(folders, vec!["Archives", "Sent"]);
    assert_eq!(dup["skew"], true); // Archives mtime is "now", message says 2021

    // ---- message without Message-ID still ingested
    let (_, body) = req(r, "GET", &search_uri("vertragsentwurf", aid), None).await;
    assert_eq!(body["total"], 1);

    // ---- broken sender clock: implausible Date rejected, falls to INTERNALDATE
    let (_, body) = req(r, "GET", &search_uri("zeitreise", aid), None).await;
    assert_eq!(body["results"][0]["date_source"], "internaldate", "{body}");

    // ---- outlook-style forward: quoted content still findable, ranked below fresh
    let (_, body) = req(r, "GET", &search_uri("dachsanierung", aid), None).await;
    assert!(body["total"].as_i64().unwrap() >= 1);

    // ---- message detail
    let (_, body) = req(r, "GET", &search_uri("klempner", aid), None).await;
    let root_id = body["results"][0]["id"].as_i64().unwrap();
    let (s, detail) = req(r, "GET", &format!("/api/message/{root_id}"), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(detail["dates"]["source"], "received");
    assert!(detail["dates"]["received_top"].as_i64().is_some());
    assert!(detail["dates"]["date_header"].as_i64().is_some());
    assert_eq!(detail["attachments"][0]["filename"], "Rechnung-Boiler.pdf");
    assert_eq!(detail["attachments"][0]["mime"], "application/octet-stream");
    assert_eq!(detail["images_allowed"], false);

    // ---- thread view in reply order
    let tid_enc = urlencode(&tid);
    let (s, thread) = req(r, "GET", &format!("/api/thread/{aid}/{tid_enc}"), None).await;
    assert_eq!(s, StatusCode::OK, "{thread}");
    let msgs = thread["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 3);
    let dates: Vec<i64> = msgs.iter().map(|m| m["date"].as_i64().unwrap()).collect();
    assert!(dates.windows(2).all(|w| w[0] <= w[1]), "thread not in date order");

    // ---- attachment bytes fetched on demand from the source
    let (s, bytes, _) = raw_req(r, &format!("/api/message/{root_id}/part/2")).await;
    assert_eq!(s, StatusCode::OK);
    assert!(bytes.starts_with(b"%PDF"), "expected decoded pdf bytes");

    // ---- html rendering: sanitised, remote images dark, cid rewritten
    let (_, body) = req(r, "GET", &search_uri("herbstaktion", aid), None).await;
    let promo_id = body["results"][0]["id"].as_i64().unwrap();
    let (s, html, headers) = raw_req(r, &format!("/api/message/{promo_id}/html")).await;
    assert_eq!(s, StatusCode::OK);
    let html = String::from_utf8(html).unwrap();
    assert!(!html.contains("<script"), "script survived sanitising");
    assert!(!html.contains("<form"), "form survived sanitising");
    assert!(!html.contains("tracker.shop.ch"), "remote image not blocked");
    assert!(html.contains(&format!("/api/message/{promo_id}/part/")), "cid not rewritten: {html}");
    assert!(headers.get("content-security-policy").is_some());
    // with images=1 the remote src stays
    let (_, html, _) = raw_req(r, &format!("/api/message/{promo_id}/html?images=1")).await;
    let html = String::from_utf8(html).unwrap();
    assert!(html.contains("tracker.shop.ch"));

    // ---- cid endpoint returns the inline png
    let (s, bytes, _) = raw_req(r, &format!("/api/message/{promo_id}/cid/logo%40shop.ch")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(&bytes[..4], b"\x89PNG");

    // ---- per-sender image allowance: grant, list, revoke
    let (s, _) = req(r, "POST", &format!("/api/message/{promo_id}/allow_images"), None).await;
    assert_eq!(s, StatusCode::OK);
    let (_, html, _) = raw_req(r, &format!("/api/message/{promo_id}/html")).await;
    assert!(String::from_utf8(html).unwrap().contains("tracker.shop.ch"));
    let (_, body) = req(r, "GET", &format!("/api/accounts/{aid}/image_allowances"), None).await;
    assert_eq!(body["allowances"][0]["sender"], "newsletter@shop.ch", "{body}");
    let (s, _) = req(
        r,
        "DELETE",
        &format!("/api/accounts/{aid}/image_allowances"),
        Some(serde_json::json!({"sender": "newsletter@shop.ch"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (_, body) = req(r, "GET", &format!("/api/accounts/{aid}/image_allowances"), None).await;
    assert_eq!(body["allowances"].as_array().unwrap().len(), 0);
    let (_, html, _) = raw_req(r, &format!("/api/message/{promo_id}/html")).await;
    assert!(!String::from_utf8(html).unwrap().contains("tracker.shop.ch"), "revoke must re-block");

    // ---- account rename
    let (s, _) = req(
        r,
        "POST",
        &format!("/api/accounts/{aid}/rename"),
        Some(serde_json::json!({"name": "renamed"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (_, body) = req(r, "GET", "/api/status", None).await;
    assert_eq!(body["accounts"][0]["name"], "renamed");
    let (s, _) = req(r, "POST", &format!("/api/accounts/{aid}/rename"), Some(serde_json::json!({"name": "  "}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // ---- raw source view
    let (s, raw, _) = raw_req(r, &format!("/api/message/{root_id}/raw")).await;
    assert_eq!(s, StatusCode::OK);
    let raw = String::from_utf8_lossy(&raw).to_string();
    assert!(raw.contains("Message-ID: <boiler-1@immo.ch>"));
    assert!(raw.contains("attachment part 2"));

    // ---- contacts and organisations
    let (_, body) = req(r, "GET", &format!("/api/contacts?account={aid}&q=hans"), None).await;
    let hans = &body["contacts"][0];
    assert_eq!(hans["email"], "hans@immo.ch");
    assert_eq!(hans["display_name"], "Hans Muster");
    assert_eq!(hans["org"], "immo.ch");
    let (_, body) = req(r, "GET", &format!("/api/contacts?account={aid}&q=newsletter"), None).await;
    assert_eq!(body["contacts"][0]["is_role"], true);
    let (_, body) = req(r, "GET", &format!("/api/contacts?account={aid}&q=anna"), None).await;
    assert_eq!(body["contacts"][0]["org"], Value::Null, "freemail must not be an org");
    let (_, body) = req(r, "GET", &format!("/api/orgs?account={aid}"), None).await;
    assert!(body["orgs"].as_array().unwrap().iter().any(|o| o["org"] == "immo.ch"));

    // ---- merge applies at query time, survives rebuild, and is undoable
    let (s, _) = req(
        r,
        "POST",
        "/api/merge",
        Some(serde_json::json!({"account": aid, "a": "hans@immo.ch", "b": "hans.muster@gmx.ch"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (_, body) = req(r, "GET", &search_uri("contact:hans.muster@gmx.ch", aid), None).await;
    assert_eq!(body["total"], 3, "merged alias must find the immo mails: {body}");

    let (s, body) = req(r, "POST", &format!("/api/accounts/{aid}/rebuild"), None).await;
    assert_eq!(s, StatusCode::OK, "{body}");
    assert_eq!(body["rebuilt"], 10);
    let (_, body) = req(r, "GET", &search_uri("contact:hans.muster@gmx.ch", aid), None).await;
    assert_eq!(body["total"], 3, "merge lost in rebuild: {body}");
    let (_, body) = req(r, "GET", &search_uri("boiler", aid), None).await;
    assert_eq!(body["total"], 3, "search broken after rebuild: {body}");

    let (s, _) = req(
        r,
        "POST",
        "/api/unmerge",
        Some(serde_json::json!({"account": aid, "a": "hans@immo.ch", "b": "hans.muster@gmx.ch"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (_, body) = req(r, "GET", &search_uri("contact:hans.muster@gmx.ch", aid), None).await;
    assert_eq!(body["total"], 0, "unmerge must cost nothing: {body}");

    // ---- re-sync: server-side deletion disappears, move updates location
    std::fs::remove_file(t.corpus.join("INBOX/007-outlook-fwd.eml")).unwrap();
    std::fs::rename(
        t.corpus.join("INBOX/006-newsletter-cid.eml"),
        t.corpus.join("Archives/006-newsletter-cid.eml"),
    )
    .unwrap();
    sync_account(&t.app, aid, None).await;

    let (_, body) = req(r, "GET", &search_uri("dachsanierung", aid), None).await;
    assert_eq!(body["total"], 0, "deleted message still in index: {body}");
    let (_, body) = req(r, "GET", &search_uri("herbstaktion", aid), None).await;
    assert_eq!(body["total"], 1);
    let folders = body["results"][0]["folders"].as_array().unwrap();
    assert_eq!(folders.len(), 1);
    assert_eq!(folders[0], "Archives", "move not reconciled: {body}");
    // attachment still fetchable after the move (stale-pointer recovery)
    let moved_id = body["results"][0]["id"].as_i64().unwrap();
    let (s, bytes, _) = raw_req(r, &format!("/api/message/{moved_id}/cid/logo%40shop.ch")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(&bytes[..4], b"\x89PNG");

    let (_, body) = req(r, "GET", "/api/status", None).await;
    assert_eq!(body["accounts"][0]["message_count"], 9);

    // ---- excluding a folder purges its already-synced mail on next sync
    let (s, _) = req(
        r,
        "PUT",
        &format!("/api/accounts/{aid}/folders"),
        Some(serde_json::json!({"excluded": ["Archives"]})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    sync_account(&t.app, aid, None).await;
    // photos + the moved newsletter lived only in Archives -> gone
    let (_, body) = req(r, "GET", &search_uri("wanderwochenende", aid), None).await;
    assert_eq!(body["total"], 0, "Archives-only mail must be purged: {body}");
    let (_, body) = req(r, "GET", &search_uri("herbstaktion", aid), None).await;
    assert_eq!(body["total"], 0, "{body}");
    // the duplicate also lives in Sent -> survives, minus its Archives pointer
    let (_, body) = req(r, "GET", &search_uri("steuererklaerung", aid), None).await;
    assert_eq!(body["total"], 1, "{body}");
    assert_eq!(
        body["results"][0]["folders"],
        serde_json::json!(["Sent"]),
        "{body}"
    );
    let (_, body) = req(r, "GET", "/api/status", None).await;
    assert_eq!(body["accounts"][0]["message_count"], 7);
}

#[tokio::test(flavor = "multi_thread")]
async fn bounded_subset_and_account_removal() {
    let tmp = tempfile::tempdir().unwrap();
    let corpus = tmp.path().join("corpus");
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/corpus");
    Command::new("cp").arg("-r").arg(&src).arg(&corpus).status().unwrap();
    let data = tmp.path().join("data");
    std::fs::create_dir_all(&data).unwrap();
    let store = Store::open(data.join("db.sqlite").to_str().unwrap()).await.unwrap();
    let app: SharedApp = Arc::new(App {
        store,
        indexes: Indexes::new(data.join("index")),
        crypto: None,
        progress: Default::default(),
    });
    let router = api::router(app.clone());
    let (_, body) = req(
        &router,
        "POST",
        "/api/accounts",
        Some(serde_json::json!({"name": "bounded", "fixture_dir": corpus.to_str().unwrap()})),
    )
    .await;
    let aid = body["id"].as_i64().unwrap();

    // bounded ingest: at most 2 per folder
    sync_account(&app, aid, Some(2)).await;
    let (_, body) = req(&router, "GET", "/api/status", None).await;
    let count = body["accounts"][0]["message_count"].as_i64().unwrap();
    assert!(count <= 5, "bounded ingest overshot: {count}");
    assert!(count >= 4, "bounded ingest undershot: {count}");

    // a later unbounded sync picks up the rest (resumability)
    sync_account(&app, aid, None).await;
    let (_, body) = req(&router, "GET", "/api/status", None).await;
    assert_eq!(body["accounts"][0]["message_count"], 10);

    // account removal deletes everything derived
    let (s, _) = req(&router, "DELETE", &format!("/api/accounts/{aid}"), None).await;
    assert_eq!(s, StatusCode::OK);
    let (_, body) = req(&router, "GET", "/api/status", None).await;
    assert_eq!(body["accounts"].as_array().unwrap().len(), 0);
}
