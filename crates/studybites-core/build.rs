//! The licensing public key is compiled in from the environment, so Cargo has
//! to be told to rebuild when it changes. Without this a cached build made
//! before the key existed would keep being reused after the secret was added,
//! and would silently ship an unlicensed app.
fn main() {
    println!("cargo:rerun-if-env-changed=STUDYBITES_LICENSE_PUBKEY");
}
