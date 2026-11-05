//! Key derivation and the symmetric primitives behind encryption at rest
//! (Brief Section 9).
//!
//! Two keys come out of one activation, both derived with HKDF-SHA256 from
//! the device identity and the secret carried in the activation code:
//!
//! ```text
//! ikm  = device_id || license_secret
//! salt = APP_SALT
//! db-key    -> the SQLCipher key
//! file-key  -> cold storage (XChaCha20-Poly1305)
//! ```
//!
//! Neither key is ever written down. They are recomputed on every launch from
//! the machine and the stored licence, so a database or an originals folder
//! copied to another PC derives a different key and simply will not open.

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::error::{CoreError, Result};

/// Fixed application salt. This is not a secret — it is a domain separator, so
/// that the same device and licence used by a different program could not
/// derive the same keys.
pub const APP_SALT: &[u8] = b"StudyBites/2026/key-derivation/v1";

/// 32 bytes of key material that scrubs itself when it goes out of scope.
#[derive(Clone, ZeroizeOnDrop)]
pub struct SecretKey([u8; 32]);

impl SecretKey {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Lowercase hex, for SQLCipher's raw-key PRAGMA form.
    ///
    /// The caller owns the returned String and must zeroize it; `Db` does.
    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }
}

// Deliberately opaque: a key must never end up in a log line or an IPC error.
impl std::fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretKey(…)")
    }
}

/// The pair of keys an activated install runs on.
#[derive(Clone, ZeroizeOnDrop)]
pub struct DeviceKeys {
    pub db: SecretKey,
    pub file: SecretKey,
}

/// Derive both keys from the device identity and the licence secret.
///
/// Same inputs always give the same keys — that is the whole point, since
/// nothing about them is stored.
pub fn derive(device_id: &[u8; 32], license_secret: &[u8]) -> DeviceKeys {
    let mut ikm = Vec::with_capacity(device_id.len() + license_secret.len());
    ikm.extend_from_slice(device_id);
    ikm.extend_from_slice(license_secret);

    let hk = Hkdf::<Sha256>::new(Some(APP_SALT), &ikm);
    ikm.zeroize();

    let mut db = [0u8; 32];
    let mut file = [0u8; 32];
    // `expand` only fails for absurd output lengths; 32 bytes cannot fail.
    hk.expand(b"db-key", &mut db).expect("hkdf db-key");
    hk.expand(b"file-key", &mut file).expect("hkdf file-key");

    let keys = DeviceKeys {
        db: SecretKey::from_bytes(db),
        file: SecretKey::from_bytes(file),
    };
    db.zeroize();
    file.zeroize();
    keys
}

// ---- authenticated encryption ------------------------------------------

/// XChaCha20-Poly1305 nonce length. 24 bytes is wide enough that random
/// nonces are safe without tracking a counter across runs.
pub const NONCE_LEN: usize = 24;

/// Encrypt `plaintext`, returning `nonce || ciphertext||tag`.
///
/// `aad` is authenticated but not encrypted — cold storage passes the file
/// header, so a file cannot be replayed under a different header.
pub fn seal(key: &SecretKey, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>> {
    let cipher = XChaCha20Poly1305::new(key.as_bytes().into());

    let mut nonce_bytes = [0u8; NONCE_LEN];
    getrandom::getrandom(&mut nonce_bytes)
        .map_err(|e| CoreError::Invalid(format!("no secure randomness available: {e}")))?;
    let nonce = XNonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(
            nonce,
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| CoreError::Invalid("could not encrypt that file".into()))?;

    let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Reverse of [`seal`]. Any tampering — with the body, the nonce or the AAD —
/// fails the Poly1305 tag rather than returning wrong plaintext.
pub fn open(key: &SecretKey, aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>> {
    if sealed.len() <= NONCE_LEN {
        return Err(CoreError::Invalid(
            "that stored file is truncated or not in a format this version understands".into(),
        ));
    }
    let cipher = XChaCha20Poly1305::new(key.as_bytes().into());
    let nonce = XNonce::from_slice(&sealed[..NONCE_LEN]);

    cipher
        .decrypt(
            nonce,
            Payload {
                msg: &sealed[NONCE_LEN..],
                aad,
            },
        )
        .map_err(|_| {
            CoreError::Invalid(
                "that stored file could not be decrypted on this device — it may have been \
                 copied from another device or altered"
                    .into(),
            )
        })
}
