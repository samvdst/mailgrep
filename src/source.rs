//! MailSource: the seam between ingestion and where mail actually lives.
//! Two implementations: IMAP (imapsource.rs) and a fixture directory of
//! .eml files (here), which is what the top-level tests drive.

use crate::types::*;
use anyhow::{Context, Result};
use mail_parser::{MessageParser, MimeHeaders, PartType};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub struct FolderListing {
    pub uidvalidity: u32,
    /// sorted descending (newest-first ingestion order)
    pub uids: Vec<u32>,
}

pub trait MailSource: Send {
    fn folders(&mut self) -> Result<Vec<String>>;
    fn enumerate(&mut self, folder: &str) -> Result<FolderListing>;
    fn fetch_text(&mut self, folder: &str, uids: &[u32]) -> Result<Vec<(u32, RawMessage)>>;
    /// Decoded bytes of one MIME part (attachment download / cid image).
    fn fetch_part(&mut self, folder: &str, uid: u32, part: &str) -> Result<Vec<u8>>;
    /// Stale-UID recovery: find a message by Message-ID anywhere.
    fn locate(&mut self, msgid: &str) -> Result<Option<(String, u32, u32)>>;
}

// ---------- .eml -> RawMessage (shared by fixtures and tests) ----------

/// Walk a parsed message assigning IMAP-style part paths, and split into
/// text part entities + attachment metadata.
pub fn eml_to_raw(bytes: &[u8], internaldate: Option<i64>) -> Result<RawMessage> {
    let msg = MessageParser::default()
        .parse(bytes)
        .context("unparseable message")?;
    let raw = msg.raw_message();
    let header_end = msg.root_part().offset_body as usize;
    let header_bytes = raw[..header_end].to_vec();

    let mut text_parts = Vec::new();
    let mut attachments = Vec::new();
    walk_parts(&msg, 0, "", bytes, &mut text_parts, &mut attachments);

    Ok(RawMessage {
        header_bytes,
        text_parts,
        attachments,
        internaldate,
        flags: vec![],
    })
}

/// (path, mime) pairs for the text parts of an eml, so the store can record
/// each stored text part's MIME type.
pub fn text_part_mimes(bytes: &[u8]) -> Vec<(String, String)> {
    let Some(msg) = MessageParser::default().parse(bytes) else {
        return vec![];
    };
    let mut text_parts: Vec<TextPartRaw> = Vec::new();
    let mut attachments = Vec::new();
    walk_parts(&msg, 0, "", bytes, &mut text_parts, &mut attachments);
    text_parts
        .iter()
        .map(|tp| {
            let mime = MessageParser::default()
                .parse(&tp.entity)
                .and_then(|m| {
                    m.content_type().map(|ct| {
                        format!(
                            "{}/{}",
                            ct.ctype(),
                            ct.subtype().unwrap_or("plain")
                        )
                    })
                })
                .unwrap_or_else(|| "text/plain".into());
            (tp.path.clone(), mime)
        })
        .collect()
}

fn walk_parts(
    msg: &mail_parser::Message,
    part_id: usize,
    prefix: &str,
    raw: &[u8],
    text_parts: &mut Vec<TextPartRaw>,
    attachments: &mut Vec<AttachMeta>,
) {
    let Some(part) = msg.parts.get(part_id) else {
        return;
    };
    match &part.body {
        PartType::Multipart(children) => {
            for (i, child) in children.iter().enumerate() {
                let path = if prefix.is_empty() {
                    format!("{}", i + 1)
                } else {
                    format!("{prefix}.{}", i + 1)
                };
                walk_parts(msg, *child as usize, &path, raw, text_parts, attachments);
            }
        }
        body => {
            let path = if prefix.is_empty() { "1".to_string() } else { prefix.to_string() };
            let is_text = matches!(body, PartType::Text(_) | PartType::Html(_));
            let disposition_attach = part
                .content_disposition()
                .map(|d| d.ctype().eq_ignore_ascii_case("attachment"))
                .unwrap_or(false);
            if is_text && !disposition_attach {
                // standalone entity: for the root part this is the whole
                // message; for children, slice the part's header..end range
                let entity = if prefix.is_empty() {
                    raw.to_vec()
                } else {
                    raw[part.offset_header as usize..part.offset_end as usize].to_vec()
                };
                text_parts.push(TextPartRaw { path, entity });
            } else {
                let mime = part
                    .content_type()
                    .map(|ct| {
                        format!("{}/{}", ct.ctype(), ct.subtype().unwrap_or("octet-stream"))
                    })
                    .unwrap_or_else(|| "application/octet-stream".into());
                let filename = part.attachment_name().map(|s| s.to_string());
                let content_id = part.content_id().map(|s| s.trim_matches(['<', '>']).to_string());
                let inline = content_id.is_some()
                    || part
                        .content_disposition()
                        .map(|d| d.ctype().eq_ignore_ascii_case("inline"))
                        .unwrap_or(false);
                attachments.push(AttachMeta {
                    path,
                    filename,
                    mime: mime.to_ascii_lowercase(),
                    size: part.contents().len() as u64,
                    content_id,
                    inline,
                });
            }
        }
    }
}

/// Decoded contents of the part at an IMAP-style path within an eml.
pub fn eml_part_contents(bytes: &[u8], path: &str) -> Result<Vec<u8>> {
    let msg = MessageParser::default()
        .parse(bytes)
        .context("unparseable message")?;
    fn descend<'a>(
        msg: &'a mail_parser::Message,
        part_id: usize,
        segments: &[usize],
    ) -> Option<usize> {
        let part = msg.parts.get(part_id)?;
        match (&part.body, segments) {
            (_, []) => Some(part_id),
            (PartType::Multipart(children), [head, rest @ ..]) => {
                descend(msg, *children.get(head - 1)? as usize, rest)
            }
            // path "1" addressing a non-multipart root
            (_, [1]) => Some(part_id),
            _ => None,
        }
    }
    let segments: Vec<usize> = path
        .split('.')
        .map(|s| s.parse::<usize>())
        .collect::<Result<_, _>>()
        .context("bad part path")?;
    let part_id = descend(&msg, 0, &segments).context("no such part")?;
    Ok(msg.parts[part_id].contents().to_vec())
}

// ---------- fixture source ----------

/// Folder per subdirectory of root (files at top level land in INBOX).
/// UIDs are stable hashes of the filename, so deleting or moving one file
/// never renumbers the others, mirroring real IMAP UID semantics.
pub struct FixtureSource {
    root: PathBuf,
}

fn uid_for(path: &PathBuf) -> u32 {
    use sha2::{Digest, Sha256};
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let h = Sha256::digest(name.as_bytes());
    u32::from_le_bytes([h[0], h[1], h[2], h[3]]).max(1)
}

impl FixtureSource {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn folder_files(&self, folder: &str) -> Result<Vec<PathBuf>> {
        let dir = if folder == "INBOX" && !self.root.join("INBOX").is_dir() {
            self.root.clone()
        } else {
            self.root.join(folder)
        };
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
            .with_context(|| format!("no fixture folder {dir:?}"))?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().map(|e| e == "eml").unwrap_or(false))
            .collect();
        files.sort();
        Ok(files)
    }

    fn file_for(&self, folder: &str, uid: u32) -> Result<PathBuf> {
        let files = self.folder_files(folder)?;
        files
            .into_iter()
            .find(|f| uid_for(f) == uid)
            .context("no such uid")
    }
}

impl MailSource for FixtureSource {
    fn folders(&mut self) -> Result<Vec<String>> {
        let mut folders: Vec<String> = std::fs::read_dir(&self.root)?
            .flatten()
            .filter(|e| e.path().is_dir())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        let has_top_level_eml = self
            .folder_files("INBOX")
            .map(|f| !f.is_empty())
            .unwrap_or(false);
        if has_top_level_eml && !folders.contains(&"INBOX".to_string()) {
            folders.push("INBOX".into());
        }
        folders.sort();
        Ok(folders)
    }

    fn enumerate(&mut self, folder: &str) -> Result<FolderListing> {
        let files = self.folder_files(folder)?;
        // newest-first by filename order (reverse lexicographic)
        let mut named: Vec<PathBuf> = files;
        named.sort();
        named.reverse();
        let uids: Vec<u32> = named.iter().map(uid_for).collect();
        Ok(FolderListing {
            uidvalidity: 1,
            uids,
        })
    }

    fn fetch_text(&mut self, folder: &str, uids: &[u32]) -> Result<Vec<(u32, RawMessage)>> {
        let mut out = Vec::new();
        for &uid in uids {
            let path = self.file_for(folder, uid)?;
            let bytes = std::fs::read(&path)?;
            let mtime = std::fs::metadata(&path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64);
            out.push((uid, eml_to_raw(&bytes, mtime)?));
        }
        Ok(out)
    }

    fn fetch_part(&mut self, folder: &str, uid: u32, part: &str) -> Result<Vec<u8>> {
        let path = self.file_for(folder, uid)?;
        let bytes = std::fs::read(&path)?;
        eml_part_contents(&bytes, part)
    }

    fn locate(&mut self, msgid: &str) -> Result<Option<(String, u32, u32)>> {
        let folders = self.folders()?;
        for folder in folders {
            let files = self.folder_files(&folder)?;
            for f in files.iter() {
                let bytes = std::fs::read(f)?;
                if let Some(m) = MessageParser::default().parse(&bytes) {
                    if let Some(id) = m.message_id() {
                        if crate::normalize::normalize_msgid(id)
                            == crate::normalize::normalize_msgid(msgid)
                        {
                            return Ok(Some((folder, uid_for(f), 1)));
                        }
                    }
                }
            }
        }
        Ok(None)
    }
}

/// Text-part mime lookup keyed by path, for a whole RawMessage whose text
/// parts came from eml_to_raw (each entity is parseable on its own).
pub fn raw_text_part_mimes(raw: &RawMessage) -> Vec<(String, String)> {
    raw.text_parts
        .iter()
        .map(|tp| {
            let mime = MessageParser::default()
                .parse(&tp.entity)
                .and_then(|m| {
                    m.content_type()
                        .map(|ct| format!("{}/{}", ct.ctype(), ct.subtype().unwrap_or("plain")))
                })
                .unwrap_or_else(|| "text/plain".into());
            (tp.path.clone(), mime)
        })
        .collect()
}

#[allow(dead_code)]
fn _unused(_: &BTreeMap<u32, u32>) {}

#[cfg(test)]
mod tests {
    use super::*;

    const MULTIPART: &[u8] = b"Message-ID: <mp@test.ch>\r\n\
From: a@b.ch\r\n\
Subject: multi\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=\"XX\"\r\n\r\n\
--XX\r\n\
Content-Type: multipart/alternative; boundary=\"YY\"\r\n\r\n\
--YY\r\n\
Content-Type: text/plain; charset=utf-8\r\n\r\n\
plain body\r\n\
--YY\r\n\
Content-Type: text/html; charset=utf-8\r\n\r\n\
<p>html body</p>\r\n\
--YY--\r\n\
--XX\r\n\
Content-Type: image/jpeg; name=\"photo.jpg\"\r\n\
Content-Disposition: attachment; filename=\"photo.jpg\"\r\n\
Content-Transfer-Encoding: base64\r\n\r\n\
/9j/4AAQ\r\n\
--XX--\r\n";

    #[test]
    fn multipart_walk_paths_and_attachments() {
        let raw = eml_to_raw(MULTIPART, None).unwrap();
        let paths: Vec<&str> = raw.text_parts.iter().map(|t| t.path.as_str()).collect();
        assert_eq!(paths, vec!["1.1", "1.2"]);
        assert_eq!(raw.attachments.len(), 1);
        assert_eq!(raw.attachments[0].path, "2");
        assert_eq!(raw.attachments[0].filename.as_deref(), Some("photo.jpg"));
        assert_eq!(raw.attachments[0].mime, "image/jpeg");
        // header block ends before the first boundary
        assert!(String::from_utf8_lossy(&raw.header_bytes).contains("Message-ID"));
    }

    #[test]
    fn part_contents_decodes_base64() {
        let bytes = eml_part_contents(MULTIPART, "2").unwrap();
        assert_eq!(&bytes[..3], &[0xFF, 0xD8, 0xFF]); // JPEG magic from /9j/
    }

    #[test]
    fn simple_message_is_path_1() {
        let eml = b"Message-ID: <s@t.ch>\r\nFrom: a@b.ch\r\nContent-Type: text/plain\r\n\r\nhello\r\n";
        let raw = eml_to_raw(eml, None).unwrap();
        assert_eq!(raw.text_parts.len(), 1);
        assert_eq!(raw.text_parts[0].path, "1");
        let c = eml_part_contents(eml, "1").unwrap();
        assert_eq!(String::from_utf8_lossy(&c).trim(), "hello");
    }
}
