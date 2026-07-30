//! Tantivy: one index per account. Fresh text ranks above quoted history
//! because we own the ranking function.

use crate::queryparse::{Clause, FilterField, Query as Ast};
use crate::store::{StoredMessage, StoredPart};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::ops::Bound;
use std::path::PathBuf;
use std::sync::Mutex;
use tantivy::collector::{Count, TopDocs};
use tantivy::query::{
    AllQuery, BooleanQuery, BoostQuery, FuzzyTermQuery, Occur, PhraseQuery, Query, RangeQuery,
    RegexQuery, TermQuery,
};
use tantivy::schema::{
    Field, IndexRecordOption, Schema, Value, FAST, INDEXED, STORED, STRING, TEXT,
};
use tantivy::{Index, IndexReader, IndexWriter, TantivyDocument, Term};

/// Boosts are configuration, not constants — they need tuning on the real corpus.
#[derive(Debug, Clone)]
pub struct RankConfig {
    pub fresh_boost: f32,
    pub quoted_boost: f32,
    pub subject_boost: f32,
    pub exact_over_fuzzy: f32,
}

impl Default for RankConfig {
    fn default() -> Self {
        Self {
            fresh_boost: env_f32("MAILGREP_BOOST_FRESH", 1.0),
            quoted_boost: env_f32("MAILGREP_BOOST_QUOTED", 0.25),
            subject_boost: env_f32("MAILGREP_BOOST_SUBJECT", 1.8),
            exact_over_fuzzy: env_f32("MAILGREP_BOOST_EXACT", 2.0),
        }
    }
}

fn env_f32(k: &str, default: f32) -> f32 {
    std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

struct Fields {
    id: Field,
    subject: Field,
    subject_norm: Field,
    fresh: Field,
    quoted: Field,
    names: Field,
    addrs: Field,
    from_email: Field,
    from_domain: Field,
    to_email: Field,
    to_domain: Field,
    cc_email: Field,
    cc_domain: Field,
    participant: Field,
    org: Field,
    folder: Field,
    thread: Field,
    ext: Field,
    mimecat: Field,
    filename: Field,
    filename_raw: Field,
    date: Field,
    sent: Field,
    stored_date: Field,
    skew: Field,
    has_attach: Field,
}

fn build_schema() -> (Schema, Fields) {
    let mut b = Schema::builder();
    let f = Fields {
        id: b.add_text_field("id", STRING | STORED),
        subject: b.add_text_field("subject", TEXT),
        subject_norm: b.add_text_field("subject_norm", TEXT),
        fresh: b.add_text_field("fresh", TEXT | STORED),
        quoted: b.add_text_field("quoted", TEXT),
        names: b.add_text_field("names", TEXT),
        addrs: b.add_text_field("addrs", TEXT),
        from_email: b.add_text_field("from_email", STRING),
        from_domain: b.add_text_field("from_domain", STRING),
        to_email: b.add_text_field("to_email", STRING),
        to_domain: b.add_text_field("to_domain", STRING),
        cc_email: b.add_text_field("cc_email", STRING),
        cc_domain: b.add_text_field("cc_domain", STRING),
        participant: b.add_text_field("participant", STRING),
        org: b.add_text_field("org", STRING),
        folder: b.add_text_field("folder", STRING),
        thread: b.add_text_field("thread", STRING),
        ext: b.add_text_field("ext", STRING),
        mimecat: b.add_text_field("mimecat", STRING),
        filename: b.add_text_field("filename", TEXT),
        filename_raw: b.add_text_field("filename_raw", STRING),
        date: b.add_i64_field("date", INDEXED | FAST | STORED),
        sent: b.add_i64_field("sent", INDEXED),
        stored_date: b.add_i64_field("stored_date", INDEXED),
        skew: b.add_u64_field("skew", INDEXED),
        has_attach: b.add_u64_field("has_attach", INDEXED),
    };
    (b.build(), f)
}

pub struct AccountIndex {
    index: Index,
    reader: IndexReader,
    writer: Mutex<IndexWriter>,
    fields: Fields,
    rank: RankConfig,
}

pub struct Hit {
    pub db_id: i64,
    pub score: f32,
    pub snippet: String,
}

#[derive(Default)]
pub struct SearchOptions {
    pub limit: usize,
    pub offset: usize,
    pub sort_by_date: bool,
    /// how many top hits to also return for facet computation
    pub facet_cap: usize,
}

pub struct SearchOutput {
    pub total: usize,
    pub page: Vec<Hit>,
    /// db ids of the top `facet_cap` hits, page included
    pub facet_ids: Vec<i64>,
}

impl AccountIndex {
    pub fn open(root: &PathBuf, account_id: i64, rank: RankConfig) -> Result<Self> {
        let dir = root.join(account_id.to_string());
        std::fs::create_dir_all(&dir)?;
        let (schema, fields) = build_schema();
        let mmap = tantivy::directory::MmapDirectory::open(&dir)?;
        let index = Index::open_or_create(mmap, schema)?;
        let writer = index.writer(64_000_000)?;
        let reader = index.reader()?;
        Ok(Self {
            index,
            reader,
            writer: Mutex::new(writer),
            fields,
            rank,
        })
    }

    pub fn add_message(
        &self,
        msg: &StoredMessage,
        parts: &[StoredPart],
        folders: &[String],
    ) -> Result<()> {
        let f = &self.fields;
        let mut doc = TantivyDocument::default();
        doc.add_text(f.id, msg.id.to_string());
        doc.add_text(f.subject, &msg.subject);
        doc.add_text(f.subject_norm, &msg.subject_norm);
        doc.add_text(f.fresh, msg.fresh_text());
        doc.add_text(f.quoted, msg.quoted_text());
        let mut names = String::new();
        let mut addrs = String::new();
        for a in msg.from.iter().chain(msg.to.iter()).chain(msg.cc.iter()) {
            if let Some(n) = &a.name {
                names.push_str(n);
                names.push(' ');
            }
            if let Some(e) = &a.email {
                addrs.push_str(e);
                addrs.push(' ');
                doc.add_text(f.participant, e);
                if let Some(org) = crate::normalize::org_for(e) {
                    doc.add_text(f.org, org);
                }
            }
        }
        doc.add_text(f.names, names);
        doc.add_text(f.addrs, addrs);
        for a in &msg.from {
            if let Some(e) = &a.email {
                doc.add_text(f.from_email, e);
                if let Some((_, d)) = e.rsplit_once('@') {
                    doc.add_text(f.from_domain, d);
                }
            }
        }
        for a in &msg.to {
            if let Some(e) = &a.email {
                doc.add_text(f.to_email, e);
                if let Some((_, d)) = e.rsplit_once('@') {
                    doc.add_text(f.to_domain, d);
                }
            }
        }
        for a in &msg.cc {
            if let Some(e) = &a.email {
                doc.add_text(f.cc_email, e);
                if let Some((_, d)) = e.rsplit_once('@') {
                    doc.add_text(f.cc_domain, d);
                }
            }
        }
        for folder in folders {
            doc.add_text(f.folder, folder.to_ascii_lowercase());
        }
        doc.add_text(f.thread, &msg.thread_id);
        let mut any_attach = false;
        for p in parts.iter().filter(|p| p.kind == "attach") {
            any_attach = true;
            let meta = crate::types::AttachMeta {
                path: p.path.clone(),
                filename: p.filename.clone(),
                mime: p.mime.clone(),
                size: p.size,
                content_id: p.content_id.clone(),
                inline: p.inline,
            };
            if let Some(ext) = meta.ext() {
                doc.add_text(f.ext, ext);
            }
            doc.add_text(f.mimecat, meta.mime_category());
            if let Some(name) = &p.filename {
                doc.add_text(f.filename, name);
                doc.add_text(f.filename_raw, name.to_ascii_lowercase());
            }
        }
        doc.add_i64(f.date, msg.date_canonical);
        if let Some(s) = msg.date_hdr {
            doc.add_i64(f.sent, s);
        }
        if let Some(s) = msg.internaldate {
            doc.add_i64(f.stored_date, s);
        }
        doc.add_u64(f.skew, msg.skew as u64);
        doc.add_u64(f.has_attach, any_attach as u64);

        let w = self.writer.lock().unwrap();
        w.delete_term(Term::from_field_text(f.id, &msg.id.to_string()));
        w.add_document(doc)?;
        Ok(())
    }

    pub fn delete_message(&self, db_id: i64) -> Result<()> {
        let w = self.writer.lock().unwrap();
        w.delete_term(Term::from_field_text(self.fields.id, &db_id.to_string()));
        Ok(())
    }

    pub fn commit(&self) -> Result<()> {
        self.writer.lock().unwrap().commit()?;
        self.reader.reload()?;
        Ok(())
    }

    pub fn wipe(&self) -> Result<()> {
        let mut w = self.writer.lock().unwrap();
        w.delete_all_documents()?;
        w.commit()?;
        self.reader.reload()?;
        Ok(())
    }

    pub fn doc_count(&self) -> u64 {
        self.reader.searcher().num_docs()
    }

    // ---------- query building ----------

    fn tokenize(&self, field: Field, text: &str) -> Vec<Term> {
        let mut tokenizer = self
            .index
            .tokenizer_for_field(field)
            .unwrap_or_else(|_| tantivy::tokenizer::TextAnalyzer::from(tantivy::tokenizer::SimpleTokenizer::default()));
        let mut stream = tokenizer.token_stream(text);
        let mut terms = Vec::new();
        while let Some(tok) = stream.next() {
            terms.push(Term::from_field_text(field, &tok.text));
        }
        terms
    }

    /// One free-text term across the searchable fields, exact boosted over fuzzy.
    fn term_query(&self, word: &str) -> Box<dyn Query> {
        let f = &self.fields;
        let r = &self.rank;
        let scored_fields: Vec<(Field, f32)> = vec![
            (f.fresh, r.fresh_boost),
            (f.quoted, r.quoted_boost),
            (f.subject, r.subject_boost),
            (f.subject_norm, r.subject_boost),
            (f.names, 1.5),
            (f.addrs, 1.0),
            (f.filename, 1.0),
        ];
        let word_lc = word.to_ascii_lowercase();
        let fuzzy_distance: u8 = match word_lc.chars().count() {
            0..=3 => 0,
            4..=7 => 1,
            _ => 2,
        };
        let mut subs: Vec<(Occur, Box<dyn Query>)> = Vec::new();
        for (field, boost) in scored_fields {
            for term in self.tokenize(field, &word_lc) {
                let exact = TermQuery::new(term.clone(), IndexRecordOption::WithFreqs);
                subs.push((
                    Occur::Should,
                    Box::new(BoostQuery::new(Box::new(exact), boost * r.exact_over_fuzzy)),
                ));
                if fuzzy_distance > 0 {
                    let fuzzy = FuzzyTermQuery::new(term, fuzzy_distance, true);
                    subs.push((
                        Occur::Should,
                        Box::new(BoostQuery::new(Box::new(fuzzy), boost)),
                    ));
                }
            }
        }
        if subs.is_empty() {
            return Box::new(BooleanQuery::new(vec![]));
        }
        Box::new(BooleanQuery::new(subs))
    }

    /// Quoted phrase: exact, never fuzzed, across fresh/quoted/subject.
    fn phrase_query(&self, text: &str) -> Box<dyn Query> {
        let f = &self.fields;
        let r = &self.rank;
        let mut subs: Vec<(Occur, Box<dyn Query>)> = Vec::new();
        for (field, boost) in [
            (f.fresh, r.fresh_boost),
            (f.quoted, r.quoted_boost),
            (f.subject, r.subject_boost),
        ] {
            let terms = self.tokenize(field, text);
            let q: Box<dyn Query> = match terms.len() {
                0 => continue,
                1 => Box::new(TermQuery::new(terms[0].clone(), IndexRecordOption::WithFreqs)),
                _ => Box::new(PhraseQuery::new(terms)),
            };
            subs.push((Occur::Should, Box::new(BoostQuery::new(q, boost))));
        }
        if subs.is_empty() {
            return Box::new(BooleanQuery::new(vec![]));
        }
        Box::new(BooleanQuery::new(subs))
    }

    fn filter_query(&self, field: &FilterField, values: &[String]) -> Result<Box<dyn Query>> {
        let f = &self.fields;
        let mut subs: Vec<(Occur, Box<dyn Query>)> = Vec::new();
        for v in values {
            let v = v.trim();
            let q: Box<dyn Query> = match field {
                FilterField::From => addr_filter(v, f.from_email, f.from_domain),
                FilterField::To => addr_filter(v, f.to_email, f.to_domain),
                FilterField::Cc => addr_filter(v, f.cc_email, f.cc_domain),
                FilterField::Contact => Box::new(TermQuery::new(
                    Term::from_field_text(f.participant, &v.to_ascii_lowercase()),
                    IndexRecordOption::Basic,
                )),
                FilterField::Org => Box::new(TermQuery::new(
                    Term::from_field_text(f.org, &v.to_ascii_lowercase()),
                    IndexRecordOption::Basic,
                )),
                FilterField::Folder => {
                    let lc = v.to_ascii_lowercase();
                    // exact folder, or any subfolder — both '/' and '.' occur
                    // as IMAP hierarchy delimiters in the wild
                    let mut subs: Vec<(Occur, Box<dyn Query>)> = vec![(
                        Occur::Should,
                        Box::new(TermQuery::new(
                            Term::from_field_text(f.folder, &lc),
                            IndexRecordOption::Basic,
                        )),
                    )];
                    for delim in ['/', '.'] {
                        subs.push((
                            Occur::Should,
                            Box::new(RangeQuery::new(
                                Bound::Included(Term::from_field_text(
                                    f.folder,
                                    &format!("{lc}{delim}"),
                                )),
                                Bound::Excluded(Term::from_field_text(
                                    f.folder,
                                    &format!("{lc}{}", (delim as u8 + 1) as char),
                                )),
                            )),
                        ));
                    }
                    Box::new(BooleanQuery::new(subs))
                }
                FilterField::Thread => Box::new(TermQuery::new(
                    Term::from_field_text(f.thread, v),
                    IndexRecordOption::Basic,
                )),
                FilterField::Before => {
                    let (start, _) = date_range(v)?;
                    Box::new(RangeQuery::new(
                        Bound::Unbounded,
                        Bound::Excluded(Term::from_field_i64(f.date, start)),
                    ))
                }
                FilterField::After => {
                    let (start, _) = date_range(v)?;
                    Box::new(RangeQuery::new(
                        Bound::Included(Term::from_field_i64(f.date, start)),
                        Bound::Unbounded,
                    ))
                }
                FilterField::Date => range_on(f.date, v)?,
                FilterField::Sent => range_on(f.sent, v)?,
                FilterField::Stored => range_on(f.stored_date, v)?,
                FilterField::Dateskew => {
                    let want = v != "false" && v != "0";
                    Box::new(TermQuery::new(
                        Term::from_field_u64(f.skew, want as u64),
                        IndexRecordOption::Basic,
                    ))
                }
                FilterField::Has => Box::new(TermQuery::new(
                    Term::from_field_u64(f.has_attach, 1),
                    IndexRecordOption::Basic,
                )),
                FilterField::Ext => Box::new(TermQuery::new(
                    Term::from_field_text(f.ext, &v.to_ascii_lowercase()),
                    IndexRecordOption::Basic,
                )),
                FilterField::Attachment => Box::new(TermQuery::new(
                    Term::from_field_text(f.mimecat, &v.to_ascii_lowercase()),
                    IndexRecordOption::Basic,
                )),
                FilterField::Filename => {
                    let pat = format!(".*{}.*", regex_escape(&v.to_ascii_lowercase()));
                    match RegexQuery::from_pattern(&pat, f.filename_raw) {
                        Ok(q) => Box::new(q),
                        Err(_) => Box::new(BooleanQuery::new(vec![])),
                    }
                }
            };
            subs.push((Occur::Should, q));
        }
        Ok(Box::new(BooleanQuery::new(subs)))
    }

    fn build_query(&self, ast: &Ast) -> Result<Box<dyn Query>> {
        let mut clauses: Vec<(Occur, Box<dyn Query>)> = Vec::new();
        let mut has_positive = false;
        for c in &ast.clauses {
            match c {
                Clause::Term { text, negated } => {
                    let q = self.term_query(text);
                    clauses.push((if *negated { Occur::MustNot } else { Occur::Must }, q));
                    has_positive |= !negated;
                }
                Clause::Phrase { text, negated } => {
                    let q = self.phrase_query(text);
                    clauses.push((if *negated { Occur::MustNot } else { Occur::Must }, q));
                    has_positive |= !negated;
                }
                Clause::Filter {
                    field,
                    values,
                    negated,
                } => {
                    let q = self.filter_query(field, values)?;
                    clauses.push((if *negated { Occur::MustNot } else { Occur::Must }, q));
                    has_positive |= !negated;
                }
            }
        }
        if !has_positive {
            clauses.push((Occur::Must, Box::new(AllQuery)));
        }
        Ok(Box::new(BooleanQuery::new(clauses)))
    }

    pub fn search(&self, ast: &Ast, opts: &SearchOptions) -> Result<SearchOutput> {
        let query = self.build_query(ast)?;
        let searcher = self.reader.searcher();
        let total = searcher.search(&query, &Count)?;
        let cap = opts.facet_cap.max(opts.offset + opts.limit).max(1);

        let addrs: Vec<(f32, tantivy::DocAddress)> = if opts.sort_by_date {
            searcher
                .search(
                    &query,
                    &TopDocs::with_limit(cap)
                        .order_by_fast_field::<i64>("date", tantivy::Order::Desc),
                )?
                .into_iter()
                .map(|(_, a)| (0.0f32, a))
                .collect()
        } else {
            searcher.search(&query, &TopDocs::with_limit(cap).order_by_score())?
        };

        let mut snippet_gen = tantivy::snippet::SnippetGenerator::create(
            &searcher,
            &query,
            self.fields.fresh,
        )
        .ok();
        if let Some(g) = snippet_gen.as_mut() {
            g.set_max_num_chars(220);
        }

        let mut facet_ids = Vec::with_capacity(addrs.len());
        let mut page = Vec::new();
        for (i, (score, addr)) in addrs.iter().enumerate() {
            let doc: TantivyDocument = searcher.doc(*addr)?;
            let id: i64 = doc
                .get_first(self.fields.id)
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse().ok())
                .context("doc without id")?;
            facet_ids.push(id);
            if i >= opts.offset && page.len() < opts.limit {
                let snippet = snippet_gen
                    .as_ref()
                    .map(|g| g.snippet_from_doc(&doc).to_html())
                    .unwrap_or_default();
                page.push(Hit {
                    db_id: id,
                    score: *score,
                    snippet,
                });
            }
        }
        Ok(SearchOutput {
            total,
            page,
            facet_ids,
        })
    }
}

fn addr_filter(v: &str, email_f: Field, domain_f: Field) -> Box<dyn Query> {
    let v = v.to_ascii_lowercase();
    if let Some(domain) = v.strip_prefix('@') {
        Box::new(TermQuery::new(
            Term::from_field_text(domain_f, domain),
            IndexRecordOption::Basic,
        ))
    } else if v.contains('@') {
        Box::new(TermQuery::new(
            Term::from_field_text(email_f, &v),
            IndexRecordOption::Basic,
        ))
    } else {
        // bare domain
        Box::new(TermQuery::new(
            Term::from_field_text(domain_f, &v),
            IndexRecordOption::Basic,
        ))
    }
}

/// "2021" | "2021-05" | "2021-05-03" | "a..b" -> [start, end) unix seconds (UTC).
pub fn date_range(v: &str) -> Result<(i64, i64)> {
    if let Some((a, b)) = v.split_once("..") {
        let (s, _) = date_range(a)?;
        let (_, e) = date_range(b)?;
        return Ok((s, e));
    }
    let parts: Vec<&str> = v.split('-').collect();
    let (y, m, d) = match parts.as_slice() {
        [y] => (y.parse::<i32>()?, None, None),
        [y, m] => (y.parse::<i32>()?, Some(m.parse::<u32>()?), None),
        [y, m, d] => (y.parse::<i32>()?, Some(m.parse::<u32>()?), Some(d.parse::<u32>()?)),
        _ => anyhow::bail!("bad date {v}"),
    };
    use chrono::NaiveDate;
    let start = NaiveDate::from_ymd_opt(y, m.unwrap_or(1), d.unwrap_or(1)).context("bad date")?;
    let end = match (m, d) {
        (None, _) => NaiveDate::from_ymd_opt(y + 1, 1, 1).context("bad date")?,
        (Some(m), None) => {
            let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
            NaiveDate::from_ymd_opt(ny, nm, 1).context("bad date")?
        }
        (Some(_), Some(_)) => start.succ_opt().context("bad date")?,
    };
    Ok((
        start.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp(),
        end.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp(),
    ))
}

fn range_on(field: Field, v: &str) -> Result<Box<dyn Query>> {
    let (start, end) = date_range(v)?;
    Ok(Box::new(RangeQuery::new(
        Bound::Included(Term::from_field_i64(field, start)),
        Bound::Excluded(Term::from_field_i64(field, end)),
    )))
}

fn regex_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if "\\.+*?()|[]{}^$#&-~".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// All per-account indexes, opened on demand.
pub struct Indexes {
    root: PathBuf,
    rank: RankConfig,
    map: Mutex<HashMap<i64, std::sync::Arc<AccountIndex>>>,
}

impl Indexes {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            rank: RankConfig::default(),
            map: Mutex::new(HashMap::new()),
        }
    }

    pub fn get(&self, account_id: i64) -> Result<std::sync::Arc<AccountIndex>> {
        let mut map = self.map.lock().unwrap();
        if let Some(idx) = map.get(&account_id) {
            return Ok(idx.clone());
        }
        let idx = std::sync::Arc::new(AccountIndex::open(
            &self.root,
            account_id,
            self.rank.clone(),
        )?);
        map.insert(account_id, idx.clone());
        Ok(idx)
    }

    pub fn drop_account(&self, account_id: i64) -> Result<()> {
        self.map.lock().unwrap().remove(&account_id);
        let dir = self.root.join(account_id.to_string());
        if dir.exists() {
            std::fs::remove_dir_all(dir)?;
        }
        Ok(())
    }

    pub fn index_size_bytes(&self, account_id: i64) -> u64 {
        let dir = self.root.join(account_id.to_string());
        walk_size(&dir)
    }
}

fn walk_size(dir: &PathBuf) -> u64 {
    let mut total = 0;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            if let Ok(md) = e.metadata() {
                if md.is_file() {
                    total += md.len();
                } else if md.is_dir() {
                    total += walk_size(&e.path());
                }
            }
        }
    }
    total
}
