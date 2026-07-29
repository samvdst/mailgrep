//! Sync engine: reconcile a MailSource against the store, newest-first,
//! batched. The store writes themselves are the checkpoint — a killed run
//! resumes by re-diffing enumerated UIDs against stored locations, so a
//! reboot costs one enumeration, not a re-download.

use crate::index::Indexes;
use crate::source::MailSource;
use crate::store::{Store, StoredLocation};
use anyhow::Result;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

const BATCH: usize = 100;

#[derive(Debug, Clone, Default, Serialize)]
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

pub type ProgressMap = Arc<Mutex<HashMap<i64, SyncProgress>>>;

pub struct SyncOutcome {
    pub new_msgs: i64,
    pub removed: i64,
    pub failed: i64,
}

/// Runs blocking (IMAP is sync); call from spawn_blocking with a runtime
/// handle for the async store.
pub fn run_sync(
    handle: tokio::runtime::Handle,
    store: &Store,
    indexes: &Indexes,
    source: &mut dyn MailSource,
    account_id: i64,
    excluded: &[String],
    max_per_folder: Option<usize>,
    progress: &ProgressMap,
) -> Result<SyncOutcome> {
    let log_id = handle.block_on(store.start_sync_log(account_id))?;
    let index = indexes.get(account_id)?;

    let mut outcome = SyncOutcome {
        new_msgs: 0,
        removed: 0,
        failed: 0,
    };
    let set_progress = |p: SyncProgress| {
        progress.lock().unwrap().insert(account_id, p);
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let folders = source.folders()?;
    let folders: Vec<String> = folders
        .into_iter()
        .filter(|f| !excluded.iter().any(|e| e.eq_ignore_ascii_case(f)))
        .collect();

    let mut prog = SyncProgress {
        running: true,
        ..Default::default()
    };

    for folder in &folders {
        prog.folder = folder.clone();
        set_progress(prog.clone());

        let listing = match source.enumerate(folder) {
            Ok(l) => l,
            Err(e) => {
                handle.block_on(store.record_failure(account_id, folder, None, &format!("enumerate: {e}"), None))?;
                outcome.failed += 1;
                continue;
            }
        };

        // UIDVALIDITY change invalidates every stored location in the folder.
        if let Some(stored_v) = handle.block_on(store.folder_uidvalidity(account_id, folder))? {
            if stored_v != listing.uidvalidity {
                tracing::warn!(folder, "UIDVALIDITY changed, invalidating folder");
                for id in handle.block_on(store.invalidate_folder(account_id, folder))? {
                    index.delete_message(id)?;
                }
            }
        }
        handle.block_on(store.set_folder_uidvalidity(account_id, folder, listing.uidvalidity))?;

        let known = handle.block_on(store.folder_uids(account_id, folder))?;
        let listed: std::collections::HashSet<u32> = listing.uids.iter().copied().collect();

        // Deletions / moves away
        for (&uid, _) in known.iter() {
            if !listed.contains(&uid) {
                if let Some(deleted_msg) = handle.block_on(store.remove_location(account_id, folder, uid))? {
                    index.delete_message(deleted_msg)?;
                    outcome.removed += 1;
                }
            }
        }

        // New UIDs, newest first (listing is already descending)
        let mut new_uids: Vec<u32> = listing
            .uids
            .iter()
            .copied()
            .filter(|u| !known.contains_key(u))
            .collect();
        if let Some(max) = max_per_folder {
            new_uids.truncate(max);
        }
        prog.discovered += new_uids.len() as u64;
        set_progress(prog.clone());

        for chunk in new_uids.chunks(BATCH) {
            let fetched = match source.fetch_text(folder, chunk) {
                Ok(f) => f,
                Err(e) => {
                    handle.block_on(store.record_failure(
                        account_id,
                        folder,
                        None,
                        &format!("fetch batch: {e}"),
                        None,
                    ))?;
                    outcome.failed += chunk.len() as i64;
                    prog.failed += chunk.len() as u64;
                    continue;
                }
            };
            for (uid, raw) in fetched {
                let n = crate::normalize::normalize(&raw, now);
                let loc = StoredLocation {
                    folder: folder.clone(),
                    uid,
                    uidvalidity: listing.uidvalidity,
                    internaldate: raw.internaldate,
                };
                let mimes = crate::source::raw_text_part_mimes(&raw);
                let res = handle.block_on(store.upsert_message(account_id, &n, &raw, &mimes, &loc));
                match res {
                    Ok((db_id, was_new)) => {
                        if was_new {
                            outcome.new_msgs += 1;
                            prog.new_msgs += 1;
                            handle.block_on(store.touch_contacts(account_id, &n))?;
                        }
                        // (re)index with current folder set either way
                        if let Ok(Some(msg)) = handle.block_on(store.message(db_id)) {
                            let parts = handle.block_on(store.parts(db_id))?;
                            let locs = handle.block_on(store.locations(db_id))?;
                            let folders: Vec<String> =
                                locs.iter().map(|l| l.folder.clone()).collect();
                            index.add_message(&msg, &parts, &folders)?;
                        }
                    }
                    Err(e) => {
                        handle.block_on(store.record_failure(
                            account_id,
                            folder,
                            Some(uid),
                            &e.to_string(),
                            Some(&raw.header_bytes),
                        ))?;
                        outcome.failed += 1;
                        prog.failed += 1;
                    }
                }
                prog.processed += 1;
            }
            // Batch boundary: commit the index. DB rows are already durable,
            // which is what makes resume-at-batch-granularity work.
            index.commit()?;
            set_progress(prog.clone());
        }
    }

    // Layer-2 passes over the whole account
    let changed = handle.block_on(store.recompute_threads(account_id))?;
    for id in changed {
        if let Some(msg) = handle.block_on(store.message(id))? {
            let parts = handle.block_on(store.parts(id))?;
            let locs = handle.block_on(store.locations(id))?;
            let folders: Vec<String> = locs.iter().map(|l| l.folder.clone()).collect();
            index.add_message(&msg, &parts, &folders)?;
        }
    }
    index.commit()?;

    handle.block_on(store.finish_sync_log(
        log_id,
        outcome.new_msgs,
        outcome.removed,
        outcome.failed,
        "done",
        &format!("folders: {}", folders.len()),
    ))?;
    handle.block_on(store.set_sync_result(account_id, "ok"))?;

    prog.running = false;
    set_progress(prog);
    Ok(outcome)
}

/// Rebuild all derived data from layer 1 — no network. User decisions
/// (merges, image allowances) are keyed on addresses and untouched.
pub async fn rebuild_account(store: &Store, indexes: &Indexes, account_id: i64) -> Result<usize> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let index = indexes.get(account_id)?;
    index.wipe()?;
    // wipe contact aggregates (derived); merges live in merge_ops and survive
    store
        .conn
        .execute("DELETE FROM contacts WHERE account_id = ?", libsql::params![account_id])
        .await?;
    let ids = store.message_ids(account_id).await?;
    let count = ids.len();
    for id in &ids {
        let raw = store.raw_message(*id).await?;
        let n = crate::normalize::normalize(&raw, now);
        store.update_derived(*id, &n).await?;
        store.touch_contacts(account_id, &n).await?;
    }
    let _ = store.recompute_threads(account_id).await?;
    for id in &ids {
        if let Some(msg) = store.message(*id).await? {
            let parts = store.parts(*id).await?;
            let locs = store.locations(*id).await?;
            let folders: Vec<String> = locs.iter().map(|l| l.folder.clone()).collect();
            index.add_message(&msg, &parts, &folders)?;
        }
    }
    index.commit()?;
    Ok(count)
}
