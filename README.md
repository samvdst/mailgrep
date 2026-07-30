# mailgrep

Self-hosted search over IMAP mail. Fast, accurate, and honest about what it knows.

Your mail clients are for reading and acting on mail. mailgrep does the one thing
none of them do well: **find things** — in an archive spanning years, folders, and
the accumulated damage of clients that disagree about dates and folder names.

- **One search box.** Free text plus filters in a single query string:
  `boiler landlord from:@immo.ch after:2021-01 has:attachment`
- **Correct dates.** Each message's date is derived from its `Received:` headers
  with corroboration voting across sources — folder moves and account imports
  can't corrupt it. Messages whose stored date disagrees are flagged (`dateskew:`).
- **Fresh text outranks quoted history.** Quoted replies and signatures are
  segmented out and indexed at low weight, so you land on the message where
  something was *said*, not the twentieth reply quoting it.
- **Threads from headers only** (`References`/`In-Reply-To`, JWZ-style) — never
  fuzzy subject matching, so unrelated "Rechnung" mails are never merged.
- **Contacts and organisations** derived from addresses; merge the several
  addresses one person has used (undoable, survives rebuilds).
- **Read-only.** mailgrep never writes to your mailbox. Ever.
- **Text-first storage.** Headers and text are stored locally (~2–4 GB for an
  11 GB mailbox); attachments are fetched from IMAP on demand when you click.
- **Safe rendering.** Sanitised HTML in a sandboxed iframe; remote images blocked
  by default (tracking pixels stay dark), per-sender allowances you can revoke.

## Self-hosting (Docker)

Requirements: Docker (or Podman) with compose, and an always-on box. mailgrep
has **no authentication** — the network is the trust boundary. Run it on a
private network (Tailscale, WireGuard, LAN) and never expose it publicly.

```sh
git clone https://github.com/samvdst/mailgrep.git
cd mailgrep

# The key that encrypts your IMAP credentials at rest. Generate once, keep it —
# losing it means re-entering your accounts.
echo "MAILGREP_KEY=$(openssl rand -hex 32)" > .env

docker compose up -d --build
```

Open `http://<host>:8025`, then:

1. **⚙ Settings → Add IMAP account** — host, port (993/TLS), username, password.
   Credentials are verified against the server before being stored, and stored
   encrypted with `MAILGREP_KEY`.
2. **Folders…** — exclude Junk/Trash before the first big sync. (Excluding a
   folder later also purges its already-ingested mail on the next sync.)
3. **Sync now** — or **Bounded sync…** first to ingest the newest N per folder
   and get a feel before committing to the full archive. Ingestion is
   newest-first and resumable: reboots cost seconds, not a re-download.

All state lives in the `mailgrep-data` volume — one directory to back up or
migrate. Scheduled syncs run per account on a configurable interval (default
hourly; set to 0 for manual-only).

### WSL2 note

If the host is Windows, keep the volume on the WSL2 ext4 filesystem (named
volumes do this by default). Never bind-mount from `/mnt/c` — small random I/O
crosses a translation layer roughly 10× slower, which is exactly the workload
of the index and database. Cap WSL2 memory in `.wslconfig`; indexing will use
whatever it's allowed.

## Query grammar

Free text and operators mix in one string. Every query is a URL — bookmark it.

| | |
|---|---|
| `from:` `to:` `cc:` | full address (`hans@immo.ch`) or bare domain (`immo.ch`, `@immo.ch`) |
| `contact:` | all mail to *and* from an address, following your merges |
| `org:` | everything involving a company's domain (freemail domains excluded) |
| `folder:` | scope to a folder, including its subfolders |
| `thread:` | one conversation |
| `date:` `before:` `after:` | against the corrected date: `2021`, `2021-05`, `2021-05-03`, `2021..2023` |
| `sent:` `stored:` | address the sender's `Date:` header / folder timestamp explicitly |
| `dateskew:true` | messages whose stored date disagrees with the derived one |
| `has:attachment` | attachment presence |
| `ext:pdf,jpg` | attachment file extension (comma = OR) |
| `attachment:image` | attachment MIME category — works when extensions lie |
| `filename:vertrag` | attachment filename substring |
| `"exact phrase"` | matched exactly, never fuzzed |
| `-term` / `-folder:X` | negation |

Unquoted terms tolerate typos (edit distance graded by length; exact matches
rank higher). Subject prefix soup (`AW: Re: WG: Fwd:`) is normalised away for
search and display.

## How it stores your mail

Two layers, one volume:

- **Layer 1 — raw.** What IMAP said, never corrected: headers, text-part bytes,
  MIME structure, folder/UID locations, and attachment *metadata* (name, type,
  size — never the bytes). Lives in `mailgrep.db` (SQLite-compatible libSQL).
- **Layer 2 — derived.** Canonical dates, threads, contacts, fresh/quoted
  segmentation, and the Tantivy full-text index (`index/<account>/`). Fully
  rebuildable from layer 1 — **Rebuild derived data** in settings re-runs the
  engine over stored mail with no network traffic. Your decisions (contact
  merges, image allowances) are keyed on addresses and survive every rebuild.

Accounts are a hard partition: separate indexes, no cross-account dedup, and
cross-account search ("All accounts") interleaves two separately-ranked lists
rather than pretending to a unified relevance.

## Configuration

| Env var | Default | Purpose |
|---|---|---|
| `MAILGREP_KEY` | – | 64 hex chars; encrypts IMAP credentials at rest (required to add accounts) |
| `MAILGREP_DATA` | `/data` (image) | state directory |
| `MAILGREP_BIND` | `0.0.0.0:8025` | listen address |
| `MAILGREP_WEB` | `/app/web/dist` | SPA assets |
| `MAILGREP_BOOST_FRESH` | `1.0` | ranking boost: fresh body text |
| `MAILGREP_BOOST_QUOTED` | `0.25` | ranking boost: quoted history |
| `MAILGREP_BOOST_SUBJECT` | `1.8` | ranking boost: subject |
| `MAILGREP_BOOST_EXACT` | `2.0` | exact-match boost over fuzzy |

Boosts are configuration because they need tuning against a real corpus:
`scripts/relevance.py queries.tsv` reports hit-rate@k and MRR for a set of
known-answer queries, so tuning is empirical rather than vibes.

## Development

```sh
cargo test                              # normaliser + parser units, API-over-fixtures integration
(cd web && npm install && npm run build)  # TypeScript SPA (esbuild, no framework)
cargo run                               # API + SPA on :8025
```

The test seams follow `SPEC.md`: almost everything is asserted through the HTTP
API against a committed corpus of adversarial `.eml` fixtures
(`fixtures/corpus/`), so internals are free to change. The normaliser and query
parser are additionally table-tested as pure functions. The query AST types are
exported to TypeScript via `ts-rs` (`cargo test` regenerates `bindings/`).

Architecture and the reasoning behind every decision: `DESIGN.md`. Scope and
user stories: `SPEC.md`.

## Non-goals (v1)

No IMAP writes (skewed dates are flagged, never "repaired"), no OCR or
attachment content indexing (Paperless-ngx owns that), no LLM query parsing,
no auth (private network only), no real-time IDLE sync. Semantic search is
designed for as a future separate mode — tables exist, nothing is generated.

## License

Private for now.
