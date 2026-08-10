use serde::{Deserialize, Serialize};

/// Layer 1: what the source gave us, never corrected.
/// Text-first fetch means we hold headers + text part entities, plus
/// attachment metadata from the structure, never attachment bytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawMessage {
    /// Full RFC822 header block bytes.
    pub header_bytes: Vec<u8>,
    /// Text parts as standalone MIME entities (part headers + CRLF + raw body),
    /// so the normaliser can decode charset/transfer-encoding uniformly.
    pub text_parts: Vec<TextPartRaw>,
    /// Attachment metadata from BODYSTRUCTURE / structure walk.
    pub attachments: Vec<AttachMeta>,
    /// INTERNALDATE of the location this was fetched from (unix seconds).
    pub internaldate: Option<i64>,
    pub flags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextPartRaw {
    /// IMAP part path, e.g. "1" or "1.2".
    pub path: String,
    pub entity: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachMeta {
    pub path: String,
    pub filename: Option<String>,
    pub mime: String,
    pub size: u64,
    pub content_id: Option<String>,
    pub inline: bool,
}

impl AttachMeta {
    pub fn ext(&self) -> Option<String> {
        let f = self.filename.as_ref()?;
        let (_, ext) = f.rsplit_once('.')?;
        if ext.is_empty() || ext.len() > 8 {
            return None;
        }
        Some(ext.to_ascii_lowercase())
    }
    /// Top-level MIME category, e.g. "image", plus "pdf" special-cased since
    /// `attachment:pdf` is a natural thing to type.
    pub fn mime_category(&self) -> String {
        let m = self.mime.to_ascii_lowercase();
        if m == "application/pdf" {
            return "pdf".into();
        }
        m.split('/').next().unwrap_or("application").to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Addr {
    pub email: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DateSource {
    Received,
    DateHeader,
    Internaldate,
    None,
}

impl DateSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            DateSource::Received => "received",
            DateSource::DateHeader => "date_header",
            DateSource::Internaldate => "internaldate",
            DateSource::None => "none",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DateDerived {
    /// Canonical instant, unix seconds UTC.
    pub canonical: i64,
    pub source: DateSource,
    /// Original UTC offset in minutes, for display in the sender's local time.
    pub offset_mins: i32,
    pub received_top: Option<i64>,
    pub date_hdr: Option<i64>,
    pub internaldate: Option<i64>,
    /// canonical vs INTERNALDATE differ by > 24h
    pub skew: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub quoted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedMessage {
    /// Normalised Message-ID if present.
    pub msgid: Option<String>,
    pub content_hash: String,
    /// Identity within the account: msgid or "sha256:<hash>" fallback.
    pub identity: String,
    pub subject: String,
    pub subject_norm: String,
    pub from: Vec<Addr>,
    pub to: Vec<Addr>,
    pub cc: Vec<Addr>,
    pub date: DateDerived,
    /// Normalised msgids from References + In-Reply-To.
    pub references: Vec<String>,
    /// Chosen body text (plain part preferred, else HTML converted).
    pub body_text: String,
    /// Fresh/quoted segmentation over body_text (byte offsets).
    pub spans: Vec<Span>,
    pub has_html: bool,
    pub attachments: Vec<AttachMeta>,
}

impl NormalizedMessage {
    pub fn fresh_text(&self) -> String {
        self.spans
            .iter()
            .filter(|s| !s.quoted)
            .map(|s| &self.body_text[s.start..s.end])
            .collect::<Vec<_>>()
            .join("\n")
    }
    pub fn quoted_text(&self) -> String {
        self.spans
            .iter()
            .filter(|s| s.quoted)
            .map(|s| &self.body_text[s.start..s.end])
            .collect::<Vec<_>>()
            .join("\n")
    }
}
