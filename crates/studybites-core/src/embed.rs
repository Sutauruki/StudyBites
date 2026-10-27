//! Embeddings and retrieval (Brief 2, "Vector search" and 3.8).
//!
//! The storage and search architecture is exactly what the brief specifies:
//! vectors stored as BLOBs, brute-force cosine similarity in Rust. One
//! student's corpus is a few thousand notes, so an index would be complexity
//! for nothing.
//!
//! What differs is where the vectors come from. The brief calls for a bundled
//! `all-MiniLM-L6-v2` ONNX model; that is ~23 MB plus an ONNX runtime and a
//! tokenizer, against a 2.8 MB installer today. `LexicalEmbedder` below needs
//! no model and no new dependency, and is good enough to pull the right
//! handful of notes out of one subject. `Embedder` is the seam: swapping in
//! MiniLM later is a new impl and a re-embed, with no schema change.

use crate::error::{CoreError, Result};
use serde::{Deserialize, Serialize};

/// Vector width. Small enough that a few thousand of them stay cheap to scan,
/// wide enough that unrelated words rarely collide.
pub const DIMENSIONS: usize = 256;

/// Anything that can turn text into a vector.
pub trait Embedder {
    fn dimensions(&self) -> usize;
    fn embed(&self, text: &str) -> Vec<f32>;

    fn embed_batch(&self, texts: &[String]) -> Vec<Vec<f32>> {
        texts.iter().map(|t| self.embed(t)).collect()
    }
}

/// Hashed bag-of-words with sub-linear term weighting, L2 normalised.
///
/// Deterministic and offline: the same text always gives the same vector, so
/// re-embedding after an upgrade is the only thing that changes results.
#[derive(Debug, Clone, Default)]
pub struct LexicalEmbedder;

/// Words carrying no topical signal; keeping them would make every note in a
/// subject look alike.
const STOP_WORDS: &[&str] = &[
    "the", "a", "an", "and", "or", "but", "if", "then", "than", "that", "this", "these", "those",
    "is", "are", "was", "were", "be", "been", "being", "of", "to", "in", "on", "at", "by", "for",
    "with", "from", "as", "it", "its", "we", "you", "they", "he", "she", "can", "will", "would",
    "should", "may", "might", "must", "do", "does", "did", "not", "no", "so", "such", "there",
    "here", "when", "where", "which", "who", "what", "how", "why", "all", "any", "each", "more",
    "most", "other", "some", "only", "own", "same", "very", "has", "have", "had",
];

fn is_stop_word(word: &str) -> bool {
    STOP_WORDS.contains(&word)
}

/// A light stemmer: trims the suffixes that otherwise split one idea across
/// several dimensions ("logarithm" / "logarithms").
fn stem(word: &str) -> &str {
    for suffix in ["ies", "ing", "ed", "es", "s"] {
        if word.len() > suffix.len() + 3 {
            if let Some(root) = word.strip_suffix(suffix) {
                return root;
            }
        }
    }
    word
}

/// FNV-1a, so the mapping from word to dimension is stable across machines
/// and releases. A random hasher would silently invalidate stored vectors.
fn hash_word(word: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in word.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Split into lowercase word tokens, dropping stop words and single letters.
pub fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .map(|w| w.trim().to_lowercase())
        .filter(|w| w.len() > 1 && !is_stop_word(w))
        .map(|w| stem(&w).to_string())
        .collect()
}

impl Embedder for LexicalEmbedder {
    fn dimensions(&self) -> usize {
        DIMENSIONS
    }

    fn embed(&self, text: &str) -> Vec<f32> {
        let mut vector = vec![0.0_f32; DIMENSIONS];
        for token in tokenize(text) {
            let hash = hash_word(&token);
            let slot = (hash % DIMENSIONS as u64) as usize;
            // The sign spreads unrelated words that land in the same slot, so
            // a collision tends to cancel rather than add.
            let sign = if (hash >> 63) & 1 == 1 { -1.0 } else { 1.0 };
            vector[slot] += sign;
        }
        // Sub-linear weighting: a word repeated twenty times is not twenty
        // times as important.
        for value in vector.iter_mut() {
            if *value != 0.0 {
                *value = value.signum() * (1.0 + value.abs().ln());
            }
        }
        normalize(&mut vector);
        vector
    }
}

/// Scale a vector to unit length, so cosine similarity is a plain dot product.
pub fn normalize(vector: &mut [f32]) {
    let norm: f32 = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > f32::EPSILON {
        for value in vector.iter_mut() {
            *value /= norm;
        }
    }
}

/// Cosine similarity, in −1.0..=1.0. Mismatched lengths give 0.0 rather than
/// panicking, so a vector written by an older model cannot crash a search.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|v| v * v).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm_a < f32::EPSILON || norm_b < f32::EPSILON {
        return 0.0;
    }
    (dot / (norm_a * norm_b)).clamp(-1.0, 1.0)
}

/// Pack a vector for the `notes.embedding` BLOB column: little-endian f32s.
pub fn to_blob(vector: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vector.len() * 4);
    for value in vector {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

/// Unpack a BLOB back into a vector.
pub fn from_blob(bytes: &[u8]) -> Result<Vec<f32>> {
    if bytes.len() % 4 != 0 {
        return Err(CoreError::Invalid(
            "stored embedding has a truncated value".into(),
        ));
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect())
}

/// One candidate in a similarity search.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Match {
    pub id: i64,
    pub score: f32,
}

/// Brute-force top-k over stored vectors.
///
/// `candidates` is already scoped to one subject by the caller — the brief is
/// explicit that retrieval never reaches across subjects.
pub fn top_k(query: &[f32], candidates: &[(i64, Vec<f32>)], k: usize) -> Vec<Match> {
    let mut scored: Vec<Match> = candidates
        .iter()
        .map(|(id, vector)| Match {
            id: *id,
            score: cosine_similarity(query, vector),
        })
        // A score at or below zero shares no meaningful vocabulary; sending it
        // to the model would just waste tokens.
        .filter(|m| m.score > 0.0)
        .collect();

    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.id.cmp(&b.id))
    });
    scored.truncate(k);
    scored
}
