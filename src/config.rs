//! Deployment-varying knobs, all from the environment, loaded once.
//! Built-in defaults aim to be internationally sensible; the _EXTRA vars
//! extend them for a user's locale without forking the lists.

use std::sync::OnceLock;

pub struct Cfg {
    /// extra freemail domains (never treated as organisations)
    pub freemail_extra: Vec<String>,
    /// extra role-address localparts (flagged non-human)
    pub role_extra: Vec<String>,
    /// extra reply/forward subject prefixes, e.g. "odp,ynt"
    pub subject_prefix_extra: Vec<String>,
    /// extra "everything after this line is quoted" markers, '|'-separated
    pub quote_marker_extra: Vec<String>,
    /// two date sources agree when within this many seconds (corroboration)
    pub date_agree_secs: i64,
    /// canonical vs INTERNALDATE difference that counts as skew
    pub skew_secs: i64,
    /// dates before this unix instant are implausible
    pub date_floor: i64,
    pub sync_batch: usize,
    pub imap_timeout_secs: u64,
}

fn list(var: &str, sep: char) -> Vec<String> {
    std::env::var(var)
        .unwrap_or_default()
        .split(sep)
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

fn num<T: std::str::FromStr>(var: &str, default: T) -> T {
    std::env::var(var).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

pub fn cfg() -> &'static Cfg {
    static CFG: OnceLock<Cfg> = OnceLock::new();
    CFG.get_or_init(|| Cfg {
        freemail_extra: list("MAILGREP_FREEMAIL_EXTRA", ','),
        role_extra: list("MAILGREP_ROLE_EXTRA", ','),
        subject_prefix_extra: list("MAILGREP_SUBJECT_PREFIXES_EXTRA", ','),
        quote_marker_extra: list("MAILGREP_QUOTE_MARKERS_EXTRA", '|'),
        date_agree_secs: num("MAILGREP_DATE_AGREE_HOURS", 48) * 3600,
        skew_secs: num("MAILGREP_SKEW_HOURS", 24) * 3600,
        date_floor: {
            let year: i64 = num("MAILGREP_DATE_FLOOR_YEAR", 1990);
            chrono::NaiveDate::from_ymd_opt(year as i32, 1, 1)
                .unwrap_or(chrono::NaiveDate::from_ymd_opt(1990, 1, 1).unwrap())
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_utc()
                .timestamp()
        },
        sync_batch: num("MAILGREP_SYNC_BATCH", 100),
        imap_timeout_secs: num("MAILGREP_IMAP_TIMEOUT_SECS", 120),
    })
}
