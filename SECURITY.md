# Security

mailgrep holds two sensitive things: your mail archive and (encrypted) IMAP
credentials. Treat any weakness in either as in scope.

## Reporting

Please report vulnerabilities privately via GitHub's **"Report a
vulnerability"** (Security tab) on this repository. You'll get an
acknowledgement within a few days. Please don't open public issues for
security problems before a fix is released.

## Design notes relevant to security

- mailgrep is **read-only against IMAP**: it never writes to your mailbox.
- Authentication is a single optional password (`MAILGREP_PASSWORD`); the
  session is a stateless HMAC-signed `HttpOnly`, `SameSite=Strict` cookie.
  Without it, the network is the only trust boundary. Either way, run it only
  on a private network (Tailscale, WireGuard, LAN). Do not expose it to the
  public internet.
- IMAP credentials are encrypted at rest (XChaCha20-Poly1305) with a key
  supplied via `MAILGREP_KEY`, never stored beside the ciphertext.
- Message HTML is sanitised server-side (ammonia) and rendered in a
  sandboxed iframe under a restrictive CSP; remote images are blocked by
  default.
