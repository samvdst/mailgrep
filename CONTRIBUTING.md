# Contributing

Thanks for your interest! A few things worth knowing before you start.

## Ground rules

- **Several non-features are deliberate decisions**, not gaps: mailgrep never
  writes to IMAP, threads come from `References`/`In-Reply-To` only (no fuzzy
  subject grouping — a correct-but-incomplete thread beats a confident wrong
  merge), quoted text is downweighted rather than deleted, dates are derived
  by corroboration between sources, and accounts are a hard partition. PRs
  that reverse one of these need a design argument, not just code.
- Bug reports: include the output of `/api/status`, relevant log lines
  (`podman/docker logs mailgrep`), and — if it's a parsing problem — a
  **redacted** `.eml` that reproduces it. Never attach real mail.

## Development

```sh
cargo test                                 # full suite, < 5 s
(cd web && npm install && npm run build)   # SPA
cargo run                                  # serves on :8025
```

Add a fixture account pointing at `fixtures/corpus/` to get test data into
a running instance.

## Testing conventions

- Most behaviour is tested **through the HTTP API** over the fixture corpus
  (`tests/api.rs`) — internals stay free to change.
- The normaliser and query parser are pure functions with table-driven unit
  tests. New parsing edge cases (quoting styles, date oddities, subject
  prefixes) belong there, usually with a new fixture `.eml`.
- Ranking quality is measured, not asserted: `scripts/relevance.py`.

## License

AGPL-3.0. By contributing you agree your contribution is licensed the same.
