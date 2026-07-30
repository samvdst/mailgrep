# Contributing

Thanks for your interest! A few things worth knowing before you start.

## Ground rules

- **Read `DESIGN.md` first.** Most "why doesn't it do X" questions are
  answered there, and several non-features are deliberate decisions
  (no IMAP writes, header-only threading, no fuzzy subject grouping).
  PRs that reverse a documented decision need a design argument, not just
  code.
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

## Testing conventions (from SPEC.md)

- Most behaviour is tested **through the HTTP API** over the fixture corpus
  (`tests/api.rs`) — internals stay free to change.
- The normaliser and query parser are pure functions with table-driven unit
  tests. New parsing edge cases (quoting styles, date oddities, subject
  prefixes) belong there, usually with a new fixture `.eml`.
- Ranking quality is measured, not asserted: `scripts/relevance.py`.

## License

AGPL-3.0. By contributing you agree your contribution is licensed the same.
