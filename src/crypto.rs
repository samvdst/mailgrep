//! IMAP credentials encrypted at rest; key comes from the environment
//! (MAILGREP_KEY, 64 hex chars) and is never persisted.

use anyhow::{Context, Result};
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};

pub struct Crypto {
    cipher: XChaCha20Poly1305,
}

impl Crypto {
    pub fn from_env() -> Result<Self> {
        let hex_key = std::env::var("MAILGREP_KEY")
            .context("MAILGREP_KEY not set (need 64 hex chars = 32 bytes)")?;
        let key = hex::decode(hex_key.trim()).context("MAILGREP_KEY is not valid hex")?;
        anyhow::ensure!(key.len() == 32, "MAILGREP_KEY must be 32 bytes (64 hex chars)");
        Ok(Self {
            cipher: XChaCha20Poly1305::new_from_slice(&key)
                .map_err(|e| anyhow::anyhow!("bad key length: {e}"))?,
        })
    }

    pub fn encrypt(&self, plaintext: &str) -> Result<Vec<u8>> {
        let nonce: [u8; 24] = rand::random();
        let ct = self
            .cipher
            .encrypt(XNonce::from_slice(&nonce), plaintext.as_bytes())
            .map_err(|e| anyhow::anyhow!("encrypt: {e}"))?;
        let mut out = nonce.to_vec();
        out.extend(ct);
        Ok(out)
    }

    pub fn decrypt(&self, data: &[u8]) -> Result<String> {
        anyhow::ensure!(data.len() > 24, "ciphertext too short");
        let (nonce, ct) = data.split_at(24);
        let pt = self
            .cipher
            .decrypt(XNonce::from_slice(nonce), ct)
            .map_err(|_| anyhow::anyhow!("decrypt failed (wrong MAILGREP_KEY?)"))?;
        Ok(String::from_utf8(pt)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        // SAFETY: test-only env mutation
        unsafe {
            std::env::set_var("MAILGREP_KEY", "11".repeat(32));
        }
        let c = Crypto::from_env().unwrap();
        let ct = c.encrypt("hunter2").unwrap();
        assert_ne!(ct, b"hunter2");
        assert_eq!(c.decrypt(&ct).unwrap(), "hunter2");
    }
}
