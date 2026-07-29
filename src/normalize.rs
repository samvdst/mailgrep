//! Pure transformation: RawMessage -> NormalizedMessage. No I/O.

use crate::types::*;
use mail_parser::{HeaderValue, MessageParser, MimeHeaders};
use sha2::{Digest, Sha256};

/// normalize(), but a panic on one adversarial message becomes an Err the
/// sync loop can record and skip (SPEC story 15: one malformed message must
/// never abort the run).
pub fn normalize_catch(raw: &RawMessage, now: i64) -> anyhow::Result<NormalizedMessage> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| normalize(raw, now))).map_err(|p| {
        let msg = p
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_else(|| "unknown panic".into());
        anyhow::anyhow!("normalise panicked: {msg}")
    })
}

pub fn normalize(raw: &RawMessage, now: i64) -> NormalizedMessage {
    let parser = MessageParser::default();
    // Parse the header block as a message (no body needed).
    let hdr_msg = parser.parse(&raw.header_bytes);

    let (msgid, references, subject, from, to, cc, date_hdr, date_offset, received_top) =
        match &hdr_msg {
            Some(m) => {
                let msgid = m.message_id().map(normalize_msgid);
                let mut refs: Vec<String> = Vec::new();
                for name in ["References", "In-Reply-To"] {
                    if let Some(v) = header_raw(m, name) {
                        for r in extract_msgids(&v) {
                            if !refs.contains(&r) {
                                refs.push(r);
                            }
                        }
                    }
                }
                let subject = m.subject().unwrap_or("").to_string();
                let (date_hdr, offset) = m
                    .date()
                    .map(|d| {
                        (
                            Some(d.to_timestamp()),
                            (d.tz_hour as i32 * 60 + d.tz_minute as i32)
                                * if d.tz_before_gmt { -1 } else { 1 },
                        )
                    })
                    .unwrap_or((None, 0));
                let received = topmost_received(m);
                (
                    msgid,
                    refs,
                    subject,
                    addr_list(m.from()),
                    addr_list(m.to()),
                    addr_list(m.cc()),
                    date_hdr,
                    offset,
                    received,
                )
            }
            None => (None, vec![], String::new(), vec![], vec![], vec![], None, 0, None),
        };

    // Decode text parts: each is a standalone MIME entity.
    let mut plain: Option<String> = None;
    let mut html: Option<String> = None;
    for tp in &raw.text_parts {
        if let Some(pm) = parser.parse(&tp.entity) {
            let is_html = pm
                .content_type()
                .map(|ct| ct.subtype().unwrap_or("") == "html")
                .unwrap_or(false);
            if is_html {
                if html.is_none() {
                    // body_text would convert to text; we want the raw html here
                    html = pm.body_html(0).map(|c| c.to_string());
                }
            } else if plain.is_none() {
                plain = pm.body_text(0).map(|c| c.to_string());
            }
        }
    }
    let has_html = html.is_some();
    let body_text = match (&plain, &html) {
        (Some(p), _) => p.clone(),
        (None, Some(h)) => html_to_text_quote_aware(h),
        (None, None) => String::new(),
    };
    let spans = segment(&body_text);

    // Content hash over a canonical subset: participants + date + subject + body.
    let mut hasher = Sha256::new();
    for a in from.iter().chain(to.iter()).chain(cc.iter()) {
        hasher.update(a.email.as_deref().unwrap_or(""));
        hasher.update(b"|");
    }
    hasher.update(date_hdr.unwrap_or(0).to_le_bytes());
    hasher.update(subject.as_bytes());
    hasher.update(body_text.as_bytes());
    let content_hash = hex::encode(hasher.finalize());
    let identity = msgid
        .clone()
        .unwrap_or_else(|| format!("sha256:{content_hash}"));

    let date = derive_date(received_top, date_hdr, raw.internaldate, date_offset, now);

    NormalizedMessage {
        msgid,
        content_hash,
        identity,
        subject_norm: normalize_subject(&subject),
        subject,
        from,
        to,
        cc,
        date,
        references,
        body_text,
        spans,
        has_html,
        attachments: raw.attachments.clone(),
    }
}

// ---------- headers ----------

fn header_raw(m: &mail_parser::Message, name: &str) -> Option<String> {
    m.headers()
        .iter()
        .find(|h| h.name().eq_ignore_ascii_case(name))
        .map(|h| match h.value() {
            HeaderValue::Text(t) => t.to_string(),
            HeaderValue::TextList(l) => l.join(" "),
            _ => String::from_utf8_lossy(
                &m.raw_message()[h.offset_start() as usize..h.offset_end() as usize],
            )
            .to_string(),
        })
}

pub fn normalize_msgid(raw: &str) -> String {
    let s = raw.trim().trim_start_matches('<').trim_end_matches('>').trim();
    match s.rsplit_once('@') {
        Some((local, domain)) => format!("{local}@{}", domain.to_ascii_lowercase()),
        None => s.to_string(),
    }
}

fn extract_msgids(v: &str) -> Vec<String> {
    let mut out = Vec::new();
    if v.contains('<') {
        let mut rest = v;
        while let Some(start) = rest.find('<') {
            let Some(end_rel) = rest[start..].find('>') else { break };
            let id = &rest[start + 1..start + end_rel];
            if id.contains('@') {
                out.push(normalize_msgid(id));
            }
            rest = &rest[start + end_rel + 1..];
        }
    } else {
        // mail-parser strips angle brackets when it parses id headers
        for tok in v.split_whitespace() {
            if tok.contains('@') {
                out.push(normalize_msgid(tok));
            }
        }
    }
    out
}

fn addr_list(a: Option<&mail_parser::Address>) -> Vec<Addr> {
    let mut out = Vec::new();
    if let Some(a) = a {
        for item in a.iter() {
            out.push(Addr {
                email: item.address().map(|e| e.trim().to_ascii_lowercase()),
                name: item.name().map(|n| n.trim().to_string()).filter(|n| !n.is_empty()),
            });
        }
    }
    out
}

/// Timestamp of the topmost (first) Received header.
fn topmost_received(m: &mail_parser::Message) -> Option<i64> {
    let h = m
        .headers()
        .iter()
        .find(|h| h.name().eq_ignore_ascii_case("received"))?;
    if let HeaderValue::Received(r) = h.value() {
        if let Some(dt) = r.date() {
            return Some(dt.to_timestamp());
        }
    }
    // Fallback: raw text after the last ';' parsed as RFC2822.
    let raw =
        String::from_utf8_lossy(&m.raw_message()[h.offset_start() as usize..h.offset_end() as usize]);
    let after = raw.rsplit_once(';')?.1;
    let cleaned = strip_comments(after).split_whitespace().collect::<Vec<_>>().join(" ");
    chrono::DateTime::parse_from_rfc2822(&cleaned)
        .ok()
        .map(|d| d.timestamp())
}

fn strip_comments(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in s.chars() {
        match c {
            '(' => depth += 1,
            ')' => {
                if depth > 0 {
                    depth -= 1
                }
            }
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

// ---------- dates ----------

const MIN_PLAUSIBLE: i64 = 631152000; // 1990-01-01

fn plausible(ts: i64, now: i64) -> bool {
    ts >= MIN_PLAUSIBLE && ts <= now + 86400
}

pub fn derive_date(
    received_top: Option<i64>,
    date_hdr: Option<i64>,
    internaldate: Option<i64>,
    offset_mins: i32,
    now: i64,
) -> DateDerived {
    let (canonical, source) = if let Some(r) = received_top.filter(|t| plausible(*t, now)) {
        (r, DateSource::Received)
    } else if let Some(d) = date_hdr.filter(|t| plausible(*t, now)) {
        (d, DateSource::DateHeader)
    } else if let Some(i) = internaldate.filter(|t| plausible(*t, now)) {
        (i, DateSource::Internaldate)
    } else {
        // Last resort: keep whatever exists even if implausible, else 0.
        (
            received_top.or(date_hdr).or(internaldate).unwrap_or(0),
            DateSource::None,
        )
    };
    let skew = internaldate
        .map(|i| (i - canonical).abs() > 86400)
        .unwrap_or(false);
    DateDerived {
        canonical,
        source,
        offset_mins,
        received_top,
        date_hdr,
        internaldate,
        skew,
    }
}

// ---------- subject ----------

pub fn normalize_subject(s: &str) -> String {
    let mut cur = s.trim();
    loop {
        let lower = cur.to_ascii_lowercase();
        let mut stripped = false;
        for p in ["re", "aw", "wg", "fw", "fwd", "antw", "sv", "vs"] {
            if lower.starts_with(p) {
                let rest = &cur[p.len()..];
                // allow "Re:", "Re[2]:", "RE :"
                let rest_trim = rest.trim_start();
                let rest2 = if let Some(r) = rest_trim.strip_prefix('[') {
                    match r.find(']') {
                        Some(i) if r[..i].chars().all(|c| c.is_ascii_digit()) => &r[i + 1..],
                        _ => rest_trim,
                    }
                } else {
                    rest_trim
                };
                if let Some(r) = rest2.trim_start().strip_prefix(':') {
                    cur = r.trim_start();
                    stripped = true;
                    break;
                }
            }
        }
        if stripped {
            continue;
        }
        // leading bracketed list tag: "[users] subject"
        if cur.starts_with('[') {
            if let Some(i) = cur.find(']') {
                let after = cur[i + 1..].trim_start();
                if !after.is_empty() {
                    cur = after;
                    continue;
                }
            }
        }
        break;
    }
    cur.trim().to_string()
}

// ---------- quote / signature segmentation ----------

/// Line-based segmentation into fresh and quoted spans over the original text.
/// Returns spans, never rewritten text.
pub fn segment(text: &str) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    let mut everything_quoted = false; // set after an "original message" style marker or sig
    let mut pos = 0;
    for line in text.split_inclusive('\n') {
        let start = pos;
        pos += line.len();
        let trimmed_end = line.trim_end_matches(['\n', '\r']);
        let end = start + trimmed_end.len();
        let t = trimmed_end.trim_start();

        let quoted = if everything_quoted {
            true
        } else if is_sig_delim(trimmed_end) || is_original_marker(t) {
            everything_quoted = true;
            true
        } else if t.starts_with('>') || is_attribution(t) {
            true
        } else {
            false
        };

        if trimmed_end.is_empty() {
            // attach blank lines to the previous span kind to keep spans chunky
            if let Some(last) = spans.last_mut() {
                last.end = end;
            }
            continue;
        }
        match spans.last_mut() {
            Some(last) if last.quoted == quoted => last.end = end,
            _ => spans.push(Span { start, end, quoted }),
        }
    }
    spans
}

fn is_sig_delim(line: &str) -> bool {
    line == "-- " || line == "--" || line == "—" || line == "-- \r"
}

fn is_original_marker(t: &str) -> bool {
    let l = t.to_ascii_lowercase();
    l.starts_with("-----original message-----")
        || l.starts_with("-----ursprüngliche nachricht-----")
        || l.starts_with("-----message d'origine-----")
        || l.starts_with("________________________________")
        || l.starts_with("---------- forwarded message")
        || l.starts_with("begin forwarded message")
        || l.starts_with("anfang der weitergeleiteten nachricht")
}

/// "On <date>, <person> wrote:" / "Am <date> schrieb <person>:" and common variants.
fn is_attribution(t: &str) -> bool {
    if t.len() > 200 {
        return false;
    }
    let l = t.to_ascii_lowercase();
    (l.starts_with("on ") && (l.ends_with("wrote:") || l.ends_with("wrote :")))
        || (l.starts_with("am ") && (l.ends_with("schrieb:") || l.contains(" schrieb ") && l.ends_with(':')))
        || (l.starts_with("le ") && l.ends_with("a écrit :"))
        || ((l.starts_with("von:") || l.starts_with("from:")) && l.contains('@'))
}

// ---------- HTML -> text, blockquote-aware ----------

/// Minimal tag stripper that prefixes blockquote content with "> " so the
/// line-based segmentation above applies uniformly.
pub fn html_to_text_quote_aware(html: &str) -> String {
    let mut out = String::new();
    let mut chars = html.char_indices().peekable();
    let mut bq_depth = 0usize;
    let mut at_line_start = true;
    let mut skip_until: Option<&str> = None; // inside <style>/<script>
    let bytes = html;

    fn push_line_prefix(out: &mut String, depth: usize) {
        for _ in 0..depth {
            out.push_str("> ");
        }
    }

    while let Some((i, c)) = chars.next() {
        if c == '<' {
            let rest = &bytes[i..];
            let end = match rest.find('>') {
                Some(e) => e,
                None => break,
            };
            let tag_full = &rest[1..end];
            let tag = tag_full
                .trim_start_matches('/')
                .split([' ', '\t', '\n', '\r'])
                .next()
                .unwrap_or("")
                .to_ascii_lowercase();
            let closing = tag_full.starts_with('/');
            if let Some(until) = skip_until {
                if closing && tag == until {
                    skip_until = None;
                }
            } else {
                match tag.as_str() {
                    "style" | "script" | "head" if !closing => skip_until = Some(match tag.as_str() {
                        "style" => "style",
                        "script" => "script",
                        _ => "head",
                    }),
                    "blockquote" => {
                        if closing {
                            bq_depth = bq_depth.saturating_sub(1);
                        } else {
                            bq_depth += 1;
                        }
                        if !out.ends_with('\n') && !out.is_empty() {
                            out.push('\n');
                        }
                        at_line_start = true;
                    }
                    "br" | "p" | "div" | "tr" | "li" | "h1" | "h2" | "h3" | "h4" | "table" => {
                        if !out.ends_with('\n') && !out.is_empty() {
                            out.push('\n');
                            at_line_start = true;
                        }
                    }
                    _ => {}
                }
            }
            // advance iterator past the tag
            while let Some(&(j, _)) = chars.peek() {
                if j <= i + end {
                    chars.next();
                } else {
                    break;
                }
            }
            continue;
        }
        if skip_until.is_some() {
            continue;
        }
        if c == '\n' || c == '\r' {
            // HTML source newlines are not layout; treat as space
            if !at_line_start && !out.ends_with(' ') {
                out.push(' ');
            }
            continue;
        }
        if at_line_start {
            if c == ' ' || c == '\t' {
                continue;
            }
            push_line_prefix(&mut out, bq_depth);
            at_line_start = false;
        }
        if c == '&' {
            let rest = &bytes[i..];
            // ';' is ASCII, so its index is always a char boundary; only a
            // nearby one counts as an entity
            if let Some(semi) = rest.find(';').filter(|s| *s <= 10) {
                let ent = &rest[1..semi];
                let decoded = match ent {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" | "#39" => Some('\''),
                    "nbsp" | "#160" => Some(' '),
                    _ => None,
                };
                if let Some(d) = decoded {
                    out.push(d);
                    while let Some(&(j, _)) = chars.peek() {
                        if j <= i + semi {
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    continue;
                }
            }
            out.push('&');
            continue;
        }
        out.push(c);
        if out.ends_with('\n') {
            at_line_start = true;
        }
    }
    // collapse 3+ blank lines
    let mut cleaned = String::with_capacity(out.len());
    let mut blank_run = 0;
    for line in out.lines() {
        if line.trim().is_empty() {
            blank_run += 1;
            if blank_run > 1 {
                continue;
            }
        } else {
            blank_run = 0;
        }
        cleaned.push_str(line.trim_end());
        cleaned.push('\n');
    }
    cleaned
}

// ---------- contacts helpers ----------

const FREEMAIL: &[&str] = &[
    "gmail.com", "googlemail.com", "gmx.de", "gmx.ch", "gmx.net", "gmx.at", "bluewin.ch",
    "hotmail.com", "hotmail.de", "hotmail.ch", "hotmail.fr", "outlook.com", "outlook.de",
    "live.com", "live.de", "msn.com", "yahoo.com", "yahoo.de", "yahoo.fr", "ymail.com",
    "web.de", "t-online.de", "freenet.de", "aol.com", "icloud.com", "me.com", "mac.com",
    "protonmail.com", "proton.me", "pm.me", "posteo.de", "mailbox.org", "fastmail.com",
    "hispeed.ch", "sunrise.ch", "swissonline.ch", "green.ch", "zoho.com", "mail.com",
];

const ROLE_LOCALPARTS: &[&str] = &[
    "noreply", "no-reply", "no_reply", "donotreply", "do-not-reply", "info", "support",
    "newsletter", "news", "bounce", "bounces", "mailer-daemon", "postmaster", "notification",
    "notifications", "notify", "alert", "alerts", "billing", "invoice", "service", "hello",
    "contact", "office", "admin", "webmaster", "marketing", "sales", "team", "reply",
    "kundenservice", "kontakt", "rechnung", "buchhaltung",
];

pub fn org_for(email: &str) -> Option<String> {
    let domain = email.rsplit_once('@')?.1.to_ascii_lowercase();
    if FREEMAIL.contains(&domain.as_str()) {
        None
    } else {
        Some(domain)
    }
}

pub fn is_role_address(email: &str) -> bool {
    let Some((local, _)) = email.split_once('@') else {
        return false;
    };
    let l = local.to_ascii_lowercase();
    ROLE_LOCALPARTS
        .iter()
        .any(|r| l == *r || l.starts_with(&format!("{r}+")) || l.starts_with(&format!("{r}-")) || l.starts_with(&format!("{r}@")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn msgid_normalisation() {
        assert_eq!(normalize_msgid("<ABC@Example.COM>"), "ABC@example.com");
        assert_eq!(normalize_msgid("  <x@y.z> "), "x@y.z");
        assert_eq!(normalize_msgid("noatsign"), "noatsign");
    }

    #[test]
    fn subject_normalisation_strips_prefix_soup() {
        assert_eq!(normalize_subject("Re: AW: WG: Rechnung Boiler"), "Rechnung Boiler");
        assert_eq!(normalize_subject("RE[2]: Fwd: hello"), "hello");
        assert_eq!(normalize_subject("[users] Re: patch"), "patch");
        assert_eq!(normalize_subject("Antw: SV: vs: x"), "x");
        assert_eq!(normalize_subject("Rechnung"), "Rechnung");
        // "Reminder" must not lose "Re"
        assert_eq!(normalize_subject("Reminder: pay up"), "Reminder: pay up");
    }

    #[test]
    fn date_precedence_and_plausibility() {
        let now = 1_753_000_000;
        let d = derive_date(Some(1_600_000_000), Some(1_599_000_000), Some(1_700_000_000), 120, now);
        assert_eq!(d.source, DateSource::Received);
        assert_eq!(d.canonical, 1_600_000_000);
        assert!(d.skew); // internaldate differs by >24h

        // implausible received (1970) falls through to Date:
        let d = derive_date(Some(0), Some(1_599_000_000), None, 0, now);
        assert_eq!(d.source, DateSource::DateHeader);

        // future received falls through
        let d = derive_date(Some(now + 900_000), None, Some(1_599_000_000), 0, now);
        assert_eq!(d.source, DateSource::Internaldate);

        // everything implausible: keep value, source none
        let d = derive_date(Some(100), None, None, 0, now);
        assert_eq!(d.source, DateSource::None);
        assert_eq!(d.canonical, 100);
    }

    #[test]
    fn segmentation_quotes_and_sig() {
        let text = "Fresh line one.\nFresh two.\n\n> quoted a\n> quoted b\nfresh again\n-- \nsig line\n";
        let spans = segment(text);
        let fresh: Vec<&str> = spans.iter().filter(|s| !s.quoted).map(|s| &text[s.start..s.end]).collect();
        let quoted: Vec<&str> = spans.iter().filter(|s| s.quoted).map(|s| &text[s.start..s.end]).collect();
        assert!(fresh.iter().any(|f| f.contains("Fresh line one")));
        assert!(fresh.iter().any(|f| f.contains("fresh again")));
        assert!(quoted.iter().any(|q| q.contains("quoted a")));
        assert!(quoted.iter().any(|q| q.contains("sig line")));
    }

    #[test]
    fn segmentation_attribution_en_de() {
        for attr in [
            "On Mon, 3 Jan 2022, Hans Muster wrote:",
            "Am 03.01.2022 um 10:00 schrieb Hans Muster:",
            "-----Original Message-----",
            "-----Ursprüngliche Nachricht-----",
        ] {
            let text = format!("hi\n{attr}\nold stuff\n");
            let spans = segment(&text);
            let quoted: String = spans
                .iter()
                .filter(|s| s.quoted)
                .map(|s| &text[s.start..s.end])
                .collect();
            assert!(quoted.contains(attr.trim()), "attr not quoted: {attr}");
        }
        // Original-message markers quote everything after them; bare "On..wrote:" only itself
        let text = "hi\n-----Original Message-----\nvery old\n";
        let spans = segment(text);
        assert!(spans.iter().filter(|s| s.quoted).map(|s| &text[s.start..s.end]).any(|q| q.contains("very old")));
    }

    #[test]
    fn html_entities_near_multibyte_chars_dont_panic() {
        // '&' followed by umlauts such that a naive 10-byte cut lands
        // mid-character (the 2026-07-30 sync panic)
        let text = html_to_text_quote_aware("<p>&szlig\u{fc}\u{fc} b&auml;r &amp; caf\u{e9}</p>");
        assert!(text.contains("caf\u{e9}"), "content lost: {text}");
        assert!(text.contains("\u{fc}\u{fc}"), "umlauts lost: {text}");
    }

    #[test]
    fn html_blockquote_becomes_quoted() {
        let html = "<div>fresh reply</div><blockquote type=\"cite\"><div>old text</div></blockquote>";
        let text = html_to_text_quote_aware(html);
        assert!(text.contains("fresh reply"));
        assert!(text.contains("> old text"));
        let spans = segment(&text);
        let quoted: String = spans.iter().filter(|s| s.quoted).map(|s| &text[s.start..s.end]).collect();
        assert!(quoted.contains("old text"));
    }

    #[test]
    fn org_and_role_detection() {
        assert_eq!(org_for("hans@immo.ch"), Some("immo.ch".into()));
        assert_eq!(org_for("hans@gmail.com"), None);
        assert!(is_role_address("noreply@shop.ch"));
        assert!(is_role_address("no-reply@shop.ch"));
        assert!(!is_role_address("hans.muster@shop.ch"));
    }

    #[test]
    fn full_normalize_plain_message() {
        let eml = b"Message-ID: <A1@Example.COM>\r\n\
Received: from mx1 (mx1.example.com) by mail.example.com; Mon, 3 Jan 2022 10:00:00 +0100\r\n\
Date: Mon, 3 Jan 2022 09:59:00 +0100\r\n\
From: Hans Muster <Hans@Immo.CH>\r\n\
To: sam@demo.example\r\n\
Subject: AW: Re: Boiler\r\n\
References: <root@x.ch> <mid@Y.CH>\r\n\
Content-Type: text/plain; charset=utf-8\r\n\r\n";
        let body = b"Content-Type: text/plain; charset=utf-8\r\n\r\nHallo Sam\r\n> alte zeile\r\n";
        let raw = RawMessage {
            header_bytes: eml.to_vec(),
            text_parts: vec![TextPartRaw { path: "1".into(), entity: body.to_vec() }],
            attachments: vec![],
            internaldate: Some(1_700_000_000),
            flags: vec![],
        };
        let n = normalize(&raw, 1_753_000_000);
        assert_eq!(n.msgid.as_deref(), Some("A1@example.com"));
        assert_eq!(n.subject_norm, "Boiler");
        assert_eq!(n.references, vec!["root@x.ch", "mid@y.ch"]);
        assert_eq!(n.from[0].email.as_deref(), Some("hans@immo.ch"));
        assert_eq!(n.date.source, DateSource::Received);
        assert!(n.date.skew); // internaldate 2023 vs received 2022
        assert!(n.fresh_text().contains("Hallo Sam"));
        assert!(n.quoted_text().contains("alte zeile"));
    }

    #[test]
    fn normalize_without_msgid_uses_hash() {
        let eml = b"From: a@b.c\r\nSubject: x\r\n\r\n";
        let raw = RawMessage {
            header_bytes: eml.to_vec(),
            text_parts: vec![],
            attachments: vec![],
            internaldate: Some(1_600_000_000),
            flags: vec![],
        };
        let n = normalize(&raw, 1_753_000_000);
        assert!(n.msgid.is_none());
        assert!(n.identity.starts_with("sha256:"));
        assert_eq!(n.date.source, DateSource::Internaldate);
    }

    #[test]
    fn attachment_meta_ext_and_category() {
        let a = AttachMeta {
            path: "2".into(),
            filename: Some("Vertrag.PDF".into()),
            mime: "application/octet-stream".into(),
            size: 1000,
            content_id: None,
            inline: false,
        };
        assert_eq!(a.ext().as_deref(), Some("pdf"));
        assert_eq!(a.mime_category(), "application");
        let b = AttachMeta {
            path: "3".into(),
            filename: None,
            mime: "application/pdf".into(),
            size: 1,
            content_id: None,
            inline: false,
        };
        assert_eq!(b.mime_category(), "pdf");
    }
}
