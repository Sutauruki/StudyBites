//! StudyBites admin tool — issues activation codes (Brief Section 8.4).
//!
//! This runs on the owner's machine and nowhere else. It holds the Ed25519
//! private key that the app's embedded public key checks against, so:
//!
//!   * the private key is generated here, written where the owner says, and
//!     **never** committed to this repository or shipped inside the app;
//!   * the app only ever verifies — nothing in `studybites-core` can sign.
//!
//! Usage:
//!
//! ```text
//!   license-tool keygen  --key owner.key
//!   license-tool issue   --key owner.key --request <REQUEST CODE> --buyer "Name"
//!   license-tool list    --key owner.key
//! ```
//!
//! `issue` appends to a ledger beside the key file, so the owner can see what
//! has been sold and re-issue a code for a student who lost theirs.

use std::path::{Path, PathBuf};

use ed25519_dalek::{Signer, SigningKey};
use studybites_core::license::{self, License, RequestCode, SECRET_LEN};

const USAGE: &str = "\
StudyBites license tool

  license-tool keygen --key <FILE>
      Create the signing key, once. Prints the public key to paste into the
      build (repository secret STUDYBITES_LICENSE_PUBKEY).

  license-tool issue --key <FILE> --request <CODE> --buyer \"<NAME>\"
      Turn a student's request code into an activation code.

  license-tool list --key <FILE>
      Show everything issued so far.

  license-tool request
      Print the request code for the machine this is run on. The app shows
      the same code on its activation screen; this is for checking by hand.

The key file is secret. Keep it backed up and never commit it.";

fn main() {
    if let Err(message) = run() {
        eprintln!("error: {message}");
        std::process::exit(1);
    }
}

/// Minimal `--flag value` parsing. A dependency-free tool is one less thing
/// for the owner to rebuild when something in the ecosystem moves.
struct Args {
    command: String,
    flags: Vec<(String, String)>,
}

impl Args {
    fn parse() -> Result<Self, String> {
        let mut raw = std::env::args().skip(1);
        let command = raw.next().unwrap_or_else(|| "help".into());
        let mut flags = Vec::new();
        while let Some(flag) = raw.next() {
            let Some(name) = flag.strip_prefix("--") else {
                return Err(format!("unexpected argument `{flag}`"));
            };
            let value = raw
                .next()
                .ok_or_else(|| format!("`--{name}` needs a value"))?;
            flags.push((name.to_string(), value));
        }
        Ok(Self { command, flags })
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.flags
            .iter()
            .find(|(f, _)| f == name)
            .map(|(_, v)| v.as_str())
    }

    fn require(&self, name: &str) -> Result<&str, String> {
        self.get(name)
            .ok_or_else(|| format!("`--{name}` is required\n\n{USAGE}"))
    }
}

fn run() -> Result<(), String> {
    let args = Args::parse()?;
    match args.command.as_str() {
        "keygen" => keygen(Path::new(args.require("key")?)),
        "issue" => issue(
            Path::new(args.require("key")?),
            args.require("request")?,
            args.require("buyer")?,
        ),
        "list" => list(Path::new(args.require("key")?)),
        "request" => request_code(),
        "help" | "--help" | "-h" => {
            println!("{USAGE}");
            Ok(())
        }
        other => Err(format!("unknown command `{other}`\n\n{USAGE}")),
    }
}

// ---- key handling -------------------------------------------------------

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn from_hex(text: &str) -> Result<[u8; 32], String> {
    let text = text.trim();
    if text.len() != 64 {
        return Err("the key file does not contain a 32-byte key".into());
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16)
            .map_err(|_| "the key file is not valid hex".to_string())?;
    }
    Ok(out)
}

fn keygen(path: &Path) -> Result<(), String> {
    if path.exists() {
        return Err(format!(
            "{} already exists. Refusing to overwrite it — issuing a new key would \
             invalidate every licence already sold.",
            path.display()
        ));
    }

    let mut seed = [0u8; 32];
    getrandom::getrandom(&mut seed).map_err(|e| format!("no secure randomness: {e}"))?;
    let signing = SigningKey::from_bytes(&seed);

    std::fs::write(path, hex(&seed)).map_err(|e| format!("could not write the key: {e}"))?;
    restrict(path);

    println!("Private key written to {}", path.display());
    println!("Keep it backed up. It is NOT in the repository and cannot be recovered.\n");
    println!("Public key (repository secret STUDYBITES_LICENSE_PUBKEY):\n");
    println!("  {}", hex(&signing.verifying_key().to_bytes()));
    Ok(())
}

/// Owner-only permissions where the OS has them. On Windows the file sits in
/// the owner's own profile, which is the practical equivalent.
#[cfg(unix)]
fn restrict(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict(_path: &Path) {}

fn load_key(path: &Path) -> Result<SigningKey, String> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        format!(
            "could not read {}: {e}\nRun `license-tool keygen --key {}` first.",
            path.display(),
            path.display()
        )
    })?;
    Ok(SigningKey::from_bytes(&from_hex(&text)?))
}

// ---- ledger -------------------------------------------------------------

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Ledger {
    issued: Vec<Entry>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct Entry {
    license_id: u64,
    buyer_name: String,
    /// Hex of the device tag, so a returning student can be matched up.
    device_tag: String,
    issued_days: u32,
    activation_code: String,
}

fn ledger_path(key: &Path) -> PathBuf {
    key.with_extension("licenses.json")
}

fn load_ledger(key: &Path) -> Ledger {
    std::fs::read(ledger_path(key))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_ledger(key: &Path, ledger: &Ledger) -> Result<(), String> {
    let json = serde_json::to_vec_pretty(ledger)
        .map_err(|e| format!("could not write the ledger: {e}"))?;
    std::fs::write(ledger_path(key), json).map_err(|e| format!("could not write the ledger: {e}"))
}

// ---- commands -----------------------------------------------------------

fn days_since_epoch() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| (d.as_secs() / 86_400) as u32)
        .unwrap_or(0)
}

fn issue(key_path: &Path, request: &str, buyer: &str) -> Result<(), String> {
    let signing = load_key(key_path)?;
    let request = RequestCode::decode(request).map_err(|e| e.to_string())?;

    let mut ledger = load_ledger(key_path);
    let tag_hex = hex(&request.device_tag);

    // Re-issuing for the same machine is normal: a student reinstalls, or
    // loses the code. Reuse the licence id so the ledger stays one row per
    // buyer rather than growing a line every time they ask.
    let license_id = ledger
        .issued
        .iter()
        .find(|e| e.device_tag == tag_hex)
        .map(|e| e.license_id)
        .unwrap_or_else(|| {
            ledger
                .issued
                .iter()
                .map(|e| e.license_id)
                .max()
                .unwrap_or(0)
                + 1
        });

    // The secret must be the same every time for a given licence, or the key
    // it derives changes and the student's existing database stops opening.
    let secret = match ledger.issued.iter().find(|e| e.license_id == license_id) {
        Some(previous) => existing_secret(previous, &signing)?,
        None => {
            let mut secret = [0u8; SECRET_LEN];
            getrandom::getrandom(&mut secret).map_err(|e| format!("no secure randomness: {e}"))?;
            secret
        }
    };

    let license = License {
        license_id,
        device_tag: request.device_tag,
        issued_days: days_since_epoch(),
        features: 0,
        secret,
        buyer_name: buyer.to_string(),
    };

    let body = license.signing_body().map_err(|e| e.to_string())?;
    let code = license
        .encode(&signing.sign(&body).to_bytes())
        .map_err(|e| e.to_string())?;

    ledger.issued.retain(|e| e.license_id != license_id);
    ledger.issued.push(Entry {
        license_id,
        buyer_name: buyer.to_string(),
        device_tag: tag_hex,
        issued_days: license.issued_days,
        activation_code: code.clone(),
    });
    ledger.issued.sort_by_key(|e| e.license_id);
    save_ledger(key_path, &ledger)?;

    println!("Licence #{license_id} for {buyer}");
    println!(
        "App version requested: {}.{}",
        request.app_major, request.app_minor
    );
    println!("\nActivation code — send this to the student:\n");
    println!("{code}");
    Ok(())
}

/// Recover the secret from a previously issued code, so a re-issue derives
/// the same keys and the student keeps their database.
fn existing_secret(entry: &Entry, signing: &SigningKey) -> Result<[u8; SECRET_LEN], String> {
    let public = signing.verifying_key().to_bytes();
    let verified = license::verify(&entry.activation_code, &public).map_err(|e| {
        format!(
            "the ledger entry for licence #{} is unreadable: {e}",
            entry.license_id
        )
    })?;
    Ok(verified.license().secret)
}

/// The request code for this machine, matching what the app's activation
/// screen shows. Useful when helping a student over the phone.
fn request_code() -> Result<(), String> {
    let profile = studybites_core::device::collect();
    let tag = profile.device_tag().map_err(|e| e.to_string())?;
    let code = RequestCode {
        device_tag: tag,
        app_major: 0,
        app_minor: 1,
    };
    println!("{}", code.encode());
    Ok(())
}

fn list(key_path: &Path) -> Result<(), String> {
    let ledger = load_ledger(key_path);
    if ledger.issued.is_empty() {
        println!("Nothing issued yet.");
        return Ok(());
    }
    println!(
        "{:<4}  {:<24}  {:<18}  Issued (day)",
        "#", "Buyer", "Device"
    );
    for entry in &ledger.issued {
        println!(
            "{:<4}  {:<24}  {:<18}  {}",
            entry.license_id, entry.buyer_name, entry.device_tag, entry.issued_days
        );
    }
    Ok(())
}
