//! Optional single-password login (MAILGREP_PASSWORD). Sessions are
//! stateless signed cookies, `<expiry>.<hmac>`, keyed on the password and
//! MAILGREP_KEY: changing either signs everyone out.

use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

pub const COOKIE: &str = "mailgrep_session";
pub const SESSION_SECS: i64 = 30 * 24 * 3600;

pub struct Auth {
    key: [u8; 32],
    password_mac: Vec<u8>,
    /// Held across failed attempts so guessing is serialised and slowed.
    pub attempts: tokio::sync::Mutex<()>,
}

impl Auth {
    /// `None` when MAILGREP_PASSWORD is unset or empty: no login required.
    pub fn from_env() -> Option<Self> {
        let password = std::env::var("MAILGREP_PASSWORD").ok().filter(|p| !p.is_empty())?;
        let secret = std::env::var("MAILGREP_KEY").unwrap_or_default();
        Some(Self::new(&password, secret.as_bytes()))
    }

    pub fn new(password: &str, secret: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(b"mailgrep-session\0");
        hasher.update(secret);
        hasher.update(b"\0");
        hasher.update(password.as_bytes());
        let key: [u8; 32] = hasher.finalize().into();
        let mut auth = Self {
            key,
            password_mac: Vec::new(),
            attempts: Default::default(),
        };
        auth.password_mac = auth.mac(b"password", password.as_bytes()).finalize().into_bytes().to_vec();
        auth
    }

    fn mac(&self, domain: &[u8], data: &[u8]) -> Hmac<Sha256> {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.key).expect("hmac takes any key length");
        mac.update(domain);
        mac.update(b"\0");
        mac.update(data);
        mac
    }

    /// Constant-time password check.
    pub fn check_password(&self, candidate: &str) -> bool {
        self.mac(b"password", candidate.as_bytes())
            .verify_slice(&self.password_mac)
            .is_ok()
    }

    pub fn issue(&self, now: i64) -> String {
        let expiry = (now + SESSION_SECS).to_string();
        let sig = self.mac(b"session", expiry.as_bytes()).finalize().into_bytes();
        format!("{expiry}.{}", hex::encode(sig))
    }

    pub fn verify(&self, token: &str, now: i64) -> bool {
        let Some((expiry, sig)) = token.split_once('.') else {
            return false;
        };
        let (Ok(exp), Ok(sig)) = (expiry.parse::<i64>(), hex::decode(sig)) else {
            return false;
        };
        exp > now && self.mac(b"session", expiry.as_bytes()).verify_slice(&sig).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_and_sessions() {
        let auth = Auth::new("hunter2", b"key");
        assert!(auth.check_password("hunter2"));
        assert!(!auth.check_password("hunter3"));
        assert!(!auth.check_password(""));

        let token = auth.issue(1_000);
        assert!(auth.verify(&token, 1_000));
        assert!(!auth.verify(&token, 1_000 + SESSION_SECS));
        assert!(!auth.verify(&token.replace('.', ".0"), 1_000));
        assert!(!auth.verify("garbage", 1_000));

        let (_, sig) = token.split_once('.').unwrap();
        assert!(!auth.verify(&format!("{}.{sig}", 1_000 + 10 * SESSION_SECS), 1_000));
        assert!(!Auth::new("other", b"key").verify(&token, 1_000));
        assert!(!Auth::new("hunter2", b"other").verify(&token, 1_000));
    }
}
