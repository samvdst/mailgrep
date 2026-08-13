use anyhow::Result;
use mailgrep::{api, crypto, index, store};
use axum::Router;
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::services::{ServeDir, ServeFile};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "mailgrep=info,tower_http=info".into()),
        )
        .init();

    let data_dir = PathBuf::from(std::env::var("MAILGREP_DATA").unwrap_or_else(|_| "./data".into()));
    std::fs::create_dir_all(&data_dir)?;
    let bind = std::env::var("MAILGREP_BIND").unwrap_or_else(|_| "0.0.0.0:8025".into());
    let web_dir = std::env::var("MAILGREP_WEB").unwrap_or_else(|_| "./web/dist".into());

    let store = store::Store::open(data_dir.join("mailgrep.db").to_str().unwrap()).await?;
    let indexes = index::Indexes::new(data_dir.join("index"));
    let crypto = match crypto::Crypto::from_env() {
        Ok(c) => Some(c),
        Err(e) => {
            tracing::warn!("credential encryption disabled: {e}");
            None
        }
    };

    let app = Arc::new(api::App {
        store,
        indexes,
        crypto,
        progress: Default::default(),
    });

    // Scheduled syncs: check every minute which accounts are due.
    {
        let app = app.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(60));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tick.tick().await;
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64;
                let accounts = match app.store.accounts().await {
                    Ok(a) => a,
                    Err(e) => {
                        tracing::error!("scheduler: {e:#}");
                        continue;
                    }
                };
                for a in accounts {
                    if a.sync_interval_mins <= 0 {
                        continue; // manual only
                    }
                    let due = a
                        .last_sync_at
                        .map(|t| now - t >= a.sync_interval_mins * 60)
                        .unwrap_or(true);
                    if due {
                        tracing::info!(account = a.id, "scheduled sync starting");
                        if let Err(e) = api::start_sync(app.clone(), a.id, None).await {
                            tracing::debug!(account = a.id, "scheduled sync skipped: {e:#}");
                        }
                    }
                }
            }
        });
    }

    let spa = ServeDir::new(&web_dir)
        .fallback(ServeFile::new(PathBuf::from(&web_dir).join("index.html")));
    let router: Router = api::router(app).fallback_service(spa);

    tracing::info!("mailgrep listening on {bind}, data in {data_dir:?}");
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    axum::serve(listener, router).await?;
    Ok(())
}
