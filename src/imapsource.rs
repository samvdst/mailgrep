//! Read-only IMAP MailSource. Text-first: BODYSTRUCTURE decides which parts
//! to fetch; attachment bytes only ever move on explicit request.

use crate::source::{FolderListing, MailSource};
use crate::types::*;
use anyhow::{Context, Result};
use imap::types::Fetch;
use imap_proto::types::{BodyStructure, ContentEncoding, MessageSection, SectionPath};
use native_tls::TlsStream;
use std::net::TcpStream;

type Session = imap::Session<TlsStream<TcpStream>>;

pub struct ImapSource {
    session: Session,
    selected: Option<String>,
}

impl ImapSource {
    pub fn connect(host: &str, port: u16, user: &str, password: &str) -> Result<Self> {
        let tls = native_tls::TlsConnector::builder().build()?;
        let client = imap::connect((host, port), host, &tls)
            .with_context(|| format!("cannot reach {host}:{port}"))?;
        let session = client
            .login(user, password)
            .map_err(|(e, _)| anyhow::anyhow!("login failed: {e}"))?;
        Ok(Self {
            session,
            selected: None,
        })
    }

    /// EXAMINE keeps everything read-only (no \Seen changes, no writes).
    fn select(&mut self, folder: &str) -> Result<u32> {
        let mb = self.session.examine(folder)?;
        self.selected = Some(folder.to_string());
        Ok(mb.uid_validity.unwrap_or(0))
    }

    fn ensure_selected(&mut self, folder: &str) -> Result<()> {
        if self.selected.as_deref() != Some(folder) {
            self.select(folder)?;
        }
        Ok(())
    }
}

/// Walk BODYSTRUCTURE assigning IMAP part paths. Returns text part paths
/// (with their transfer encoding) and attachment metadata.
fn walk_bodystructure(
    bs: &BodyStructure,
    prefix: &str,
    text_paths: &mut Vec<String>,
    attachments: &mut Vec<AttachMeta>,
) {
    match bs {
        BodyStructure::Multipart { bodies, .. } => {
            for (i, child) in bodies.iter().enumerate() {
                let path = if prefix.is_empty() {
                    format!("{}", i + 1)
                } else {
                    format!("{prefix}.{}", i + 1)
                };
                walk_bodystructure(child, &path, text_paths, attachments);
            }
        }
        BodyStructure::Text { common, other, .. } => {
            let path = if prefix.is_empty() { "1".into() } else { prefix.to_string() };
            let disposition_attach = common
                .disposition
                .as_ref()
                .map(|d| d.ty.eq_ignore_ascii_case("attachment"))
                .unwrap_or(false);
            if disposition_attach {
                attachments.push(attach_meta(bs, &path, common, Some(other)));
            } else {
                text_paths.push(path);
            }
        }
        BodyStructure::Basic { common, other, .. } => {
            let path = if prefix.is_empty() { "1".into() } else { prefix.to_string() };
            attachments.push(attach_meta(bs, &path, common, Some(other)));
        }
        BodyStructure::Message { common, other, .. } => {
            // embedded message: treat as attachment leaf
            let path = if prefix.is_empty() { "1".into() } else { prefix.to_string() };
            attachments.push(attach_meta(bs, &path, common, Some(other)));
        }
    }
}

fn attach_meta(
    _bs: &BodyStructure,
    path: &str,
    common: &imap_proto::types::BodyContentCommon,
    other: Option<&imap_proto::types::BodyContentSinglePart>,
) -> AttachMeta {
    fn param(params: &Option<Vec<(&str, &str)>>, key: &str) -> Option<String> {
        params.as_ref().and_then(|ps| {
            ps.iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v.to_string())
        })
    }
    let filename = common
        .disposition
        .as_ref()
        .and_then(|d| param(&d.params, "filename"))
        .or_else(|| param(&common.ty.params, "name"));
    let inline = common
        .disposition
        .as_ref()
        .map(|d| d.ty.eq_ignore_ascii_case("inline"))
        .unwrap_or(false);
    let content_id = other
        .and_then(|o| o.id.as_ref())
        .map(|s| s.trim_matches(['<', '>']).to_string());
    AttachMeta {
        path: path.to_string(),
        filename,
        mime: format!("{}/{}", common.ty.ty, common.ty.subtype).to_ascii_lowercase(),
        size: other.map(|o| o.octets as u64).unwrap_or(0),
        content_id: content_id.clone(),
        inline: inline || content_id.is_some(),
    }
}

/// Find the transfer encoding of the part at `path` in a BODYSTRUCTURE.
fn encoding_at<'a>(bs: &'a BodyStructure, path: &str) -> Option<&'a ContentEncoding<'a>> {
    fn descend<'a>(bs: &'a BodyStructure, segs: &[usize]) -> Option<&'a BodyStructure<'a>> {
        match (bs, segs) {
            (_, []) => Some(bs),
            (BodyStructure::Multipart { bodies, .. }, [h, rest @ ..]) => {
                descend(bodies.get(h - 1)?, rest)
            }
            (_, [1]) => Some(bs),
            _ => None,
        }
    }
    let segs: Vec<usize> = path.split('.').filter_map(|s| s.parse().ok()).collect();
    match descend(bs, &segs)? {
        BodyStructure::Basic { other, .. }
        | BodyStructure::Text { other, .. }
        | BodyStructure::Message { other, .. } => Some(&other.transfer_encoding),
        BodyStructure::Multipart { .. } => None,
    }
}

fn decode_transfer(data: &[u8], enc: Option<&ContentEncoding>) -> Vec<u8> {
    match enc {
        Some(ContentEncoding::Base64) => {
            mail_parser::decoders::base64::base64_decode(data).unwrap_or_else(|| data.to_vec())
        }
        Some(ContentEncoding::QuotedPrintable) => {
            mail_parser::decoders::quoted_printable::quoted_printable_decode(data)
                .unwrap_or_else(|| data.to_vec())
        }
        _ => data.to_vec(),
    }
}

fn section_path(path: &str, section: Option<MessageSection>) -> SectionPath {
    let segs: Vec<u32> = path.split('.').filter_map(|s| s.parse().ok()).collect();
    SectionPath::Part(segs, section)
}

fn fetch_uid(f: &Fetch) -> Option<u32> {
    f.uid
}

impl MailSource for ImapSource {
    fn folders(&mut self) -> Result<Vec<String>> {
        let names = self.session.list(Some(""), Some("*"))?;
        Ok(names
            .iter()
            .filter(|n| {
                !n.attributes()
                    .iter()
                    .any(|a| matches!(a, imap::types::NameAttribute::NoSelect))
            })
            .map(|n| n.name().to_string())
            .collect())
    }

    fn enumerate(&mut self, folder: &str) -> Result<FolderListing> {
        let uidvalidity = self.select(folder)?;
        let uids_set = self.session.uid_search("ALL")?;
        let mut uids: Vec<u32> = uids_set.into_iter().collect();
        uids.sort_unstable_by(|a, b| b.cmp(a)); // newest-first
        Ok(FolderListing { uidvalidity, uids })
    }

    fn fetch_text(&mut self, folder: &str, uids: &[u32]) -> Result<Vec<(u32, RawMessage)>> {
        if uids.is_empty() {
            return Ok(vec![]);
        }
        self.ensure_selected(folder)?;
        let set = uids
            .iter()
            .map(|u| u.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let fetches = self
            .session
            .uid_fetch(&set, "(UID FLAGS INTERNALDATE BODYSTRUCTURE BODY.PEEK[HEADER])")?;

        let mut out = Vec::new();
        for f in fetches.iter() {
            let Some(uid) = fetch_uid(f) else { continue };
            let header_bytes = f.header().unwrap_or_default().to_vec();
            let internaldate = f.internal_date().map(|d| d.timestamp());
            let flags: Vec<String> = f.flags().iter().map(|fl| format!("{fl:?}")).collect();

            let mut text_paths = Vec::new();
            let mut attachments = Vec::new();
            if let Some(bs) = f.bodystructure() {
                walk_bodystructure(bs, "", &mut text_paths, &mut attachments);
            }

            // Fetch each text part body + its MIME headers to build a
            // standalone decodable entity.
            let mut text_parts = Vec::new();
            if !text_paths.is_empty() {
                let items = text_paths
                    .iter()
                    .map(|p| format!("BODY.PEEK[{p}] BODY.PEEK[{p}.MIME]"))
                    .collect::<Vec<_>>()
                    .join(" ");
                let part_fetches = self
                    .session
                    .uid_fetch(uid.to_string(), format!("({items})"))?;
                for pf in part_fetches.iter() {
                    if fetch_uid(pf) != Some(uid) {
                        continue;
                    }
                    for p in &text_paths {
                        let body = pf.section(&section_path(p, None));
                        let mime = pf.section(&section_path(p, Some(MessageSection::Mime)));
                        if let (Some(body), Some(mime)) = (body, mime) {
                            let mut entity = mime.to_vec();
                            entity.extend_from_slice(body);
                            text_parts.push(TextPartRaw {
                                path: p.clone(),
                                entity,
                            });
                        }
                    }
                }
            }

            out.push((
                uid,
                RawMessage {
                    header_bytes,
                    text_parts,
                    attachments,
                    internaldate,
                    flags,
                },
            ));
        }
        Ok(out)
    }

    fn fetch_part(&mut self, folder: &str, uid: u32, part: &str) -> Result<Vec<u8>> {
        self.ensure_selected(folder)?;
        let fetches = self
            .session
            .uid_fetch(uid.to_string(), format!("(UID BODYSTRUCTURE BODY.PEEK[{part}])"))?;
        let f = fetches
            .iter()
            .find(|f| fetch_uid(f) == Some(uid))
            .context("message gone")?;
        let data = f
            .section(&section_path(part, None))
            .context("part not returned")?;
        let enc = f.bodystructure().and_then(|bs| encoding_at(bs, part));
        Ok(decode_transfer(data, enc))
    }

    fn locate(&mut self, msgid: &str) -> Result<Option<(String, u32, u32)>> {
        let folders = self.folders()?;
        for folder in folders {
            let uidvalidity = match self.select(&folder) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let query = format!("HEADER Message-ID \"{}\"", msgid.replace('"', ""));
            if let Ok(uids) = self.session.uid_search(&query) {
                if let Some(uid) = uids.into_iter().next() {
                    return Ok(Some((folder, uid, uidvalidity)));
                }
            }
        }
        Ok(None)
    }
}
