//! Structure-aware chunking (Brief 3.4 step 2).
//!
//! Splits extracted text into roughly 300–600 token pieces, preferring to break
//! at headings and paragraph boundaries rather than mid-sentence, with a small
//! overlap so an idea spanning a boundary is not lost.
//!
//! Token counts are estimated, not tokenized: a tokenizer would be another
//! dependency and several MB, and the only thing riding on the number is how
//! much text goes into one AI request. Roughly four characters per token is
//! close enough for English prose and formulas.

use serde::{Deserialize, Serialize};

pub const TARGET_TOKENS: usize = 450;
pub const MAX_TOKENS: usize = 600;
pub const MIN_TOKENS: usize = 60;
pub const OVERLAP_TOKENS: usize = 50;

/// Estimated tokens for a piece of text.
pub fn estimate_tokens(text: &str) -> usize {
    text.len().div_ceil(4)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Chunk {
    /// Position in the document, starting at 0.
    pub index: usize,
    /// The nearest heading above this chunk, when the document had one.
    pub heading: Option<String>,
    pub text: String,
    pub estimated_tokens: usize,
}

/// A paragraph or heading, as found in the source.
#[derive(Debug, Clone, PartialEq)]
enum Block {
    Heading(String),
    Paragraph(String),
}

/// True for lines that look like a heading: Markdown `#`, an all-caps line, or
/// a short numbered line such as "3.1 Laws of logarithms".
fn looks_like_heading(line: &str) -> bool {
    let line = line.trim();
    if line.is_empty() || line.len() > 120 {
        return false;
    }
    if line.starts_with('#') {
        return true;
    }
    let letters: String = line.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.len() >= 3 && letters.chars().all(|c| c.is_uppercase()) {
        return true;
    }
    // "2." / "3.1" / "CHAPTER 4" style openers, with no sentence-ending period.
    let first = line.split_whitespace().next().unwrap_or("");
    let numbered = first
        .trim_end_matches('.')
        .split('.')
        .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
    numbered && !line.ends_with('.') && line.split_whitespace().count() <= 12
}

fn split_blocks(text: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut paragraph: Vec<&str> = Vec::new();

    fn flush(paragraph: &mut Vec<&str>, blocks: &mut Vec<Block>) {
        if !paragraph.is_empty() {
            let joined = paragraph.join(" ").trim().to_string();
            if !joined.is_empty() {
                blocks.push(Block::Paragraph(joined));
            }
            paragraph.clear();
        }
    }

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            flush(&mut paragraph, &mut blocks);
        } else if looks_like_heading(trimmed) {
            flush(&mut paragraph, &mut blocks);
            let title = trimmed.trim_start_matches('#').trim().to_string();
            if !title.is_empty() {
                blocks.push(Block::Heading(title));
            }
        } else {
            paragraph.push(trimmed);
        }
    }
    flush(&mut paragraph, &mut blocks);
    blocks
}

/// Largest char boundary at or below `byte`.
fn char_boundary(text: &str, byte: usize) -> usize {
    let mut cut = byte.min(text.len());
    while cut > 0 && !text.is_char_boundary(cut) {
        cut -= 1;
    }
    cut
}

fn split_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        current.push(ch);
        if matches!(ch, '.' | '!' | '?') && estimate_tokens(&current) > 8 {
            sentences.push(std::mem::take(&mut current));
        }
    }
    if !current.trim().is_empty() {
        sentences.push(current);
    }
    sentences
}

/// Break a paragraph that is too large on its own, at sentence boundaries.
fn split_long_paragraph(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();

    for sentence in split_sentences(text) {
        if !current.is_empty()
            && estimate_tokens(&current) + estimate_tokens(&sentence) > MAX_TOKENS
        {
            out.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(sentence.trim());

        // A single sentence longer than the ceiling: hard-split it.
        while estimate_tokens(&current) > MAX_TOKENS {
            let cut = char_boundary(&current, MAX_TOKENS * 4);
            let head = current[..cut].to_string();
            let tail = current[cut..].to_string();
            out.push(head);
            current = tail.trim_start().to_string();
        }
    }
    if !current.trim().is_empty() {
        out.push(current);
    }
    out
}

/// Take the last `OVERLAP_TOKENS` worth of text, starting at a word boundary.
fn tail_overlap(text: &str) -> String {
    let want = OVERLAP_TOKENS * 4;
    if text.len() <= want {
        return text.to_string();
    }
    let start = char_boundary(text, text.len() - want);
    let slice = &text[start..];
    match slice.find(char::is_whitespace) {
        Some(space) => slice[space..].trim_start().to_string(),
        None => slice.to_string(),
    }
}

fn push_chunk(chunks: &mut Vec<Chunk>, heading: &Option<String>, buffer: &mut String) {
    let body = buffer.trim();
    if !body.is_empty() {
        chunks.push(Chunk {
            index: chunks.len(),
            heading: heading.clone(),
            estimated_tokens: estimate_tokens(body),
            text: body.to_string(),
        });
    }
    buffer.clear();
}

/// Split `text` into chunks, tagging each with the heading it sits under.
pub fn chunk_text(text: &str) -> Vec<Chunk> {
    let blocks = split_blocks(text);
    let mut chunks: Vec<Chunk> = Vec::new();
    let mut heading: Option<String> = None;
    let mut buffer_heading: Option<String> = None;
    let mut buffer = String::new();

    for block in blocks {
        match block {
            Block::Heading(title) => {
                // A new heading starts a new chunk, so a chunk never straddles
                // two topics — which matters because chunks get topic-tagged.
                push_chunk(&mut chunks, &buffer_heading, &mut buffer);
                heading = Some(title);
                buffer_heading = heading.clone();
            }
            Block::Paragraph(para) => {
                if buffer.is_empty() {
                    buffer_heading = heading.clone();
                }
                let pieces = if estimate_tokens(&para) > MAX_TOKENS {
                    split_long_paragraph(&para)
                } else {
                    vec![para]
                };

                for piece in pieces {
                    let combined = estimate_tokens(&buffer) + estimate_tokens(&piece);
                    if !buffer.is_empty() && combined > TARGET_TOKENS {
                        let overlap = tail_overlap(&buffer);
                        push_chunk(&mut chunks, &buffer_heading, &mut buffer);
                        buffer_heading = heading.clone();
                        if !overlap.is_empty() && estimate_tokens(&piece) < TARGET_TOKENS {
                            buffer.push_str(&overlap);
                            buffer.push(' ');
                        }
                    }
                    if !buffer.is_empty() {
                        buffer.push_str("\n\n");
                    }
                    buffer.push_str(&piece);
                }
            }
        }
    }
    push_chunk(&mut chunks, &buffer_heading, &mut buffer);

    // A tiny trailing chunk is noise and costs a whole AI request, so fold it
    // back into the one before it when they share a heading.
    if chunks.len() >= 2 {
        let last = chunks.last().expect("len checked");
        let prev = &chunks[chunks.len() - 2];
        if last.estimated_tokens < MIN_TOKENS && last.heading == prev.heading {
            let last = chunks.pop().expect("len checked");
            let prev = chunks.last_mut().expect("len checked");
            prev.text.push_str("\n\n");
            prev.text.push_str(&last.text);
            prev.estimated_tokens = estimate_tokens(&prev.text);
        }
    }

    chunks
}
