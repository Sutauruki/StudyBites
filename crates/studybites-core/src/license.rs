//! Activation codes and licence verification (Brief Section 8.2).
//!
//! The shape of the thing:
//!
//! ```text
//! student's PC                owner's PC (license-tool)
//! ────────────                ─────────────────────────
//! request code  ──────────▶   issue(request, buyer name)
//!  (21 chars)                        │  signs with the Ed25519 private key
//!                                    ▼
//! verify + store ◀──────────  activation code
//! ```
//!
//! Codes are compact binary rather than JSON so that they stay short enough to
//! read out loud. An Ed25519 signature is 64 bytes on its own, which already
//! costs 103 characters, so every other field is kept to the minimum that
//! still does its job.
//!
//! Nothing here touches the network: activation works with the machine
//! offline, which is the point.

use ed25519_dalek::{Signature, VerifyingKey, SIGNATURE_LENGTH};
use zeroize::Zeroize;

use crate::base32;
use crate::error::{CoreError, Result};

/// Wire version of both codes. Bump when the layout changes.
pub const FORMAT_VERSION: u8 = 1;

/// Bytes of licence secret mixed into the key derivation. 16 random bytes is
/// well past what HKDF needs on top of the device id.
pub const SECRET_LEN: usize = 16;

const REQUEST_MAGIC: &[u8; 2] = b"SR";
const ACTIVATION_MAGIC: &[u8; 2] = b"SA";
const TAG_LEN: usize = 8;
const CHECKSUM_LEN: usize = 2;
/// Offset of `buyer_name` within the signed body.
const NAME_OFFSET: usize = 2 + 1 + 8 + TAG_LEN + 4 + 2 + SECRET_LEN + 1;

fn checksum(body: &[u8]) -> [u8; CHECKSUM_LEN] {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(body);
    [digest[0], digest[1]]
}

fn malformed(what: &str) -> CoreError {
    CoreError::Invalid(format!(
        "that {what} is not valid. Check it was typed correctly — the letters I, L and O are \
         read as 1, 1 and 0, so they are safe to mix up."
    ))
}

// ---- request code -------------------------------------------------------

/// What the student reads out to the owner. Carries the device tag and the
/// app version, so the owner can see what they are issuing for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestCode {
    pub device_tag: [u8; TAG_LEN],
    pub app_major: u8,
    pub app_minor: u8,
}

impl RequestCode {
    pub fn encode(&self) -> String {
        let mut body = Vec::with_capacity(13);
        body.extend_from_slice(REQUEST_MAGIC);
        body.push(FORMAT_VERSION);
        body.extend_from_slice(&self.device_tag);
        body.push(self.app_major);
        body.push(self.app_minor);
        let sum = checksum(&body);
        body.extend_from_slice(&sum);
        base32::encode_grouped(&body)
    }

    pub fn decode(text: &str) -> Result<Self> {
        let bytes = base32::decode(text).ok_or_else(|| malformed("request code"))?;
        // base32 rounds up to whole characters, so a trailing zero byte is
        // normal; compare on the prefix we actually wrote.
        let expected = 2 + 1 + TAG_LEN + 2 + CHECKSUM_LEN;
        if bytes.len() < expected {
            return Err(malformed("request code"));
        }
        let bytes = &bytes[..expected];

        if &bytes[..2] != REQUEST_MAGIC {
            return Err(malformed("request code"));
        }
        if bytes[2] != FORMAT_VERSION {
            return Err(CoreError::Invalid(
                "that request code came from a different version of StudyBites.".into(),
            ));
        }
        let split = expected - CHECKSUM_LEN;
        if checksum(&bytes[..split]) != bytes[split..] {
            return Err(malformed("request code"));
        }

        let mut device_tag = [0u8; TAG_LEN];
        device_tag.copy_from_slice(&bytes[3..3 + TAG_LEN]);
        Ok(Self {
            device_tag,
            app_major: bytes[3 + TAG_LEN],
            app_minor: bytes[4 + TAG_LEN],
        })
    }
}

// ---- activation code ----------------------------------------------------

/// The signed grant. `secret` is the half of the key material that does not
/// come from the machine, so this struct is sensitive: it is zeroized on drop
/// and never serialised anywhere the WebView can see.
#[derive(Clone)]
pub struct License {
    pub license_id: u64,
    pub device_tag: [u8; TAG_LEN],
    /// Days since the Unix epoch, which is all the precision a licence needs.
    pub issued_days: u32,
    pub features: u16,
    pub secret: [u8; SECRET_LEN],
    pub buyer_name: String,
}

impl Drop for License {
    fn drop(&mut self) {
        self.secret.zeroize();
    }
}

impl std::fmt::Debug for License {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("License")
            .field("license_id", &self.license_id)
            .field("buyer_name", &self.buyer_name)
            .field("secret", &"…")
            .finish()
    }
}

impl License {
    /// The exact bytes a signature covers.
    pub fn signing_body(&self) -> Result<Vec<u8>> {
        let name = self.buyer_name.trim();
        let name_bytes = name.as_bytes();
        if name.is_empty() {
            return Err(CoreError::Invalid("a buyer name is required".into()));
        }
        if name_bytes.len() > u8::MAX as usize {
            return Err(CoreError::Invalid("that buyer name is too long".into()));
        }

        let mut body = Vec::with_capacity(NAME_OFFSET + name_bytes.len());
        body.extend_from_slice(ACTIVATION_MAGIC);
        body.push(FORMAT_VERSION);
        body.extend_from_slice(&self.license_id.to_le_bytes());
        body.extend_from_slice(&self.device_tag);
        body.extend_from_slice(&self.issued_days.to_le_bytes());
        body.extend_from_slice(&self.features.to_le_bytes());
        body.extend_from_slice(&self.secret);
        body.push(name_bytes.len() as u8);
        body.extend_from_slice(name_bytes);
        Ok(body)
    }

    /// Render `body || signature` as a typeable code. The signature is made by
    /// the owner's tool; this crate only ever verifies.
    pub fn encode(&self, signature: &[u8; SIGNATURE_LENGTH]) -> Result<String> {
        let mut bytes = self.signing_body()?;
        bytes.extend_from_slice(signature);
        Ok(base32::encode_grouped(&bytes))
    }
}

/// A licence that has been checked against the embedded public key.
///
/// The only way to get one is [`verify`], so holding a `VerifiedLicense` *is*
/// the proof — there is no path that forgets to check the signature.
pub struct VerifiedLicense(License);

impl VerifiedLicense {
    pub fn license(&self) -> &License {
        &self.0
    }
    pub fn into_inner(self) -> License {
        self.0
    }
}

/// Parse an activation code and verify its signature against `public_key`.
///
/// This does **not** check that the licence is for this machine — that is
/// [`VerifiedLicense::matches_device`], kept separate so the owner's tool can
/// reuse the parser.
pub fn verify(code: &str, public_key: &[u8; 32]) -> Result<VerifiedLicense> {
    let bytes = base32::decode(code).ok_or_else(|| malformed("activation code"))?;
    if bytes.len() < NAME_OFFSET + SIGNATURE_LENGTH {
        return Err(malformed("activation code"));
    }
    if &bytes[..2] != ACTIVATION_MAGIC {
        return Err(malformed("activation code"));
    }
    if bytes[2] != FORMAT_VERSION {
        return Err(CoreError::Invalid(
            "that activation code was made for a different version of StudyBites.".into(),
        ));
    }

    let name_len = bytes[NAME_OFFSET - 1] as usize;
    let body_len = NAME_OFFSET + name_len;
    if bytes.len() < body_len + SIGNATURE_LENGTH {
        return Err(malformed("activation code"));
    }
    let (body, rest) = bytes.split_at(body_len);
    let signature: [u8; SIGNATURE_LENGTH] = rest[..SIGNATURE_LENGTH]
        .try_into()
        .map_err(|_| malformed("activation code"))?;

    let key = VerifyingKey::from_bytes(public_key)
        .map_err(|_| CoreError::Invalid("this build has an unusable licensing key".into()))?;

    // `verify_strict` rejects the small-order and non-canonical keys that make
    // a signature verify under more than one public key.
    key.verify_strict(body, &Signature::from_bytes(&signature))
        .map_err(|_| {
            CoreError::Invalid(
                "that activation code was not issued for StudyBites, or it has been altered."
                    .into(),
            )
        })?;

    let buyer_name = std::str::from_utf8(&body[NAME_OFFSET..])
        .map_err(|_| malformed("activation code"))?
        .to_string();

    let mut device_tag = [0u8; TAG_LEN];
    device_tag.copy_from_slice(&body[11..11 + TAG_LEN]);
    // Layout, little-endian: magic 0..2, version 2, license_id 3..11,
    // device_tag 11..19, issued_days 19..23, features 23..25,
    // secret 25..41, name_len 41, buyer_name 42..
    let mut secret = [0u8; SECRET_LEN];
    secret.copy_from_slice(&body[25..25 + SECRET_LEN]);

    Ok(VerifiedLicense(License {
        license_id: u64::from_le_bytes(body[3..11].try_into().expect("8 bytes")),
        device_tag,
        issued_days: u32::from_le_bytes(body[19..23].try_into().expect("4 bytes")),
        features: u16::from_le_bytes(body[23..25].try_into().expect("2 bytes")),
        secret,
        buyer_name,
    }))
}

impl VerifiedLicense {
    /// Is this licence for the machine it is being used on?
    ///
    /// A valid signature for somebody else's PC is still a refusal — this is
    /// what makes copying the app folder pointless.
    pub fn matches_device(&self, device_tag: &[u8; TAG_LEN]) -> bool {
        // Constant time is overkill against an attacker who owns the machine,
        // but it costs nothing and keeps the comparison honest.
        self.0
            .device_tag
            .iter()
            .zip(device_tag)
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
    }
}

/// Parse the compiled-in public key, if this build has one.
///
/// Builds without `STUDYBITES_LICENSE_PUBKEY` set run unlicensed — see
/// [`crate::license::licensing_enabled`].
pub fn embedded_public_key() -> Option<[u8; 32]> {
    let hex = option_env!("STUDYBITES_LICENSE_PUBKEY")?.trim();
    if hex.len() != 64 {
        return None;
    }
    let mut key = [0u8; 32];
    for (i, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(key)
}

/// Whether this build enforces activation at all.
///
/// A build made without the public key — a local `cargo run`, or CI before the
/// owner has added the secret — stays usable so development is not blocked.
/// It says so plainly in Settings rather than pretending to be licensed.
pub fn licensing_enabled() -> bool {
    embedded_public_key().is_some()
}

// ---- what is kept on disk ----------------------------------------------

/// The activation code, verbatim as the student entered it.
///
/// Plain text on purpose: it is signed, so altering it only makes it fail
/// verification, and it is useless on any other machine. Keeping it readable
/// means a student who reinstalls can recover their own code from the folder
/// instead of asking for it again.
pub const LICENSE_FILE: &str = "license.txt";

/// The component profile as it was at activation, for drift reporting only.
pub const PROFILE_FILE: &str = "activation.json";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoredProfile {
    pub components: Vec<crate::device::Component>,
    pub activated_at: String,
}

/// Write the activation code and the machine profile beside the database.
pub fn save(app_data: &std::path::Path, code: &str, profile: &StoredProfile) -> Result<()> {
    std::fs::create_dir_all(app_data)?;
    std::fs::write(app_data.join(LICENSE_FILE), code.as_bytes())?;
    std::fs::write(
        app_data.join(PROFILE_FILE),
        serde_json::to_vec_pretty(profile)?,
    )?;
    Ok(())
}

/// Read back the stored activation code, if this install has one.
pub fn stored(app_data: &std::path::Path) -> Option<String> {
    let text = std::fs::read_to_string(app_data.join(LICENSE_FILE)).ok()?;
    let text = text.trim().to_string();
    (!text.is_empty()).then_some(text)
}

pub fn stored_profile(app_data: &std::path::Path) -> Option<StoredProfile> {
    let bytes = std::fs::read(app_data.join(PROFILE_FILE)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

// ---- EULA acceptance ----------------------------------------------------

/// Record that this user accepted a version of the agreement.
///
/// Acceptances accumulate: a revised agreement adds a row rather than
/// replacing the record of what was agreed before.
pub fn record_eula(db: &crate::Db, version: &str, accepted_at: &str) -> Result<()> {
    db.conn().execute(
        "INSERT INTO eula_acceptance(version, accepted_at) VALUES (?1, ?2)",
        (version, accepted_at),
    )?;
    Ok(())
}

/// Has this version of the agreement been accepted already?
pub fn eula_accepted(db: &crate::Db, version: &str) -> Result<bool> {
    let count: i64 = db.conn().query_row(
        "SELECT count(*) FROM eula_acceptance WHERE version = ?1",
        [version],
        |r| r.get(0),
    )?;
    Ok(count > 0)
}
