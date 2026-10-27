//! Crockford base32 — the alphabet activation codes are written in.
//!
//! Chosen over plain base32 or hex because a student may have to read a code
//! off a phone screen and type it: the alphabet omits I, L, O and U, so the
//! classic 1/I, 0/O confusions cannot arise, and decoding is case-insensitive
//! and forgiving of the usual mistypes.

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Encode bytes, grouped into blocks of five with dashes so the eye can keep
/// its place. [`decode`] ignores the grouping.
pub fn encode_grouped(bytes: &[u8]) -> String {
    let raw = encode(bytes);
    let mut out = String::with_capacity(raw.len() + raw.len() / 5);
    for (i, ch) in raw.chars().enumerate() {
        if i > 0 && i % 5 == 0 {
            out.push('-');
        }
        out.push(ch);
    }
    out
}

pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(5) * 8);
    let mut buffer: u16 = 0;
    let mut bits: u8 = 0;

    for &byte in bytes {
        buffer = (buffer << 8) | byte as u16;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            let index = ((buffer >> bits) & 0b1_1111) as usize;
            out.push(ALPHABET[index] as char);
        }
    }
    // Pad the last partial group with zero bits rather than '=' characters;
    // the payload carries its own length, so there is nothing to disambiguate.
    if bits > 0 {
        let index = ((buffer << (5 - bits)) & 0b1_1111) as usize;
        out.push(ALPHABET[index] as char);
    }
    out
}

/// Decode, ignoring dashes, spaces and case, and accepting the substitutions
/// Crockford defines: I and L read as 1, O reads as 0.
pub fn decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 5 / 8);
    let mut buffer: u16 = 0;
    let mut bits: u8 = 0;

    for ch in text.chars() {
        if ch == '-' || ch.is_whitespace() {
            continue;
        }
        let value = match ch.to_ascii_uppercase() {
            'O' => 0,
            'I' | 'L' => 1,
            // U is not in the alphabet and has no sensible substitution.
            'U' => return None,
            c => ALPHABET.iter().position(|&a| a == c as u8)? as u16,
        };

        buffer = (buffer << 5) | value;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }

    // Whatever is left is the zero padding `encode` added. Anything else means
    // the code was truncated or has had characters dropped.
    if bits >= 5 || (buffer & ((1 << bits) - 1)) != 0 {
        return None;
    }
    Some(out)
}
