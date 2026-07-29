# mailgrep

Self-hosted search over IMAP mail. See `DESIGN.md` and `SPEC.md`.

## Run (docker/podman)

```sh
export MAILGREP_KEY=$(openssl rand -hex 32)   # encrypts IMAP creds at rest — keep it
docker compose up -d --build                  # or: podman build -t mailgrep . && podman run ...
```

Open http://host:8025, add an account in ⚙ settings (credentials are verified
before being stored), hit "Sync now". All state lives in the `/data` volume.

## Env

| Var | Default | |
|---|---|---|
| `MAILGREP_KEY` | – | 64 hex chars; required to store IMAP accounts |
| `MAILGREP_DATA` | `./data` | state directory (db + tantivy indexes) |
| `MAILGREP_BIND` | `0.0.0.0:8025` | listen address |
| `MAILGREP_WEB` | `./web/dist` | SPA assets |
| `MAILGREP_BOOST_FRESH` / `_QUOTED` / `_SUBJECT` / `_EXACT` | 1.0 / 0.25 / 1.8 / 2.0 | ranking boosts (tune on real corpus) |

## Dev

```sh
cargo test                      # unit (normaliser, parser) + API-over-fixtures integration
(cd web && npm install && npm run build)
cargo run                       # serves API + SPA on :8025
```

Query grammar: free text plus `from: to: cc: folder: org: contact: thread:
before: after: date: sent: stored: dateskew: has:attachment ext: attachment:
filename:`; `"quoted phrases"` are exact, `-` negates, commas are OR
(`ext:pdf,jpg`), dates are `2021`, `2021-05`, `2021-05-03` or `2021..2023`.

No auth — bind it to the tailnet only (DESIGN.md §13).
