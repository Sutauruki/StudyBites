//! Reading the student's uploads, and keeping the originals (Brief 3.4, 3.10, 9).
//!
//! Two jobs:
//!
//! * **Extract** readable text from PDF, DOCX and plain text.
//! * **Cold-store** the original, compressed, under a name derived from its
//!   hash. It is never touched during normal use — only for "Download
//!   original" and for re-processing later with a better model.
//!
//! Cold storage is compressed and then sealed with the device file key, under
//! its own format marker so that files written before encryption existed are
//! still readable. Extraction works in memory, so no plaintext temp file is
//! ever written.

use crate::crypto::{self, SecretKey};
use crate::error::{CoreError, Result};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Refuse anything larger than this. A lecture PDF is a few MB; something
/// much bigger is a mistake, and would stall the machine.
pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// The usual cause of an empty PDF, and image support is a later phase, so say
/// so rather than leaving the student guessing.
const NO_TEXT_IN_PDF: &str = "no text could be read from that PDF. If it is a scan or photographs of pages, image support is coming in a later version.";

/// What kind of file we were given, decided by content and extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Pdf,
    Docx,
    Text,
}

impl FileKind {
    pub fn mime(self) -> &'static str {
        match self {
            Self::Pdf => "application/pdf",
            Self::Docx => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            Self::Text => "text/plain",
        }
    }

    /// Detect from the file's own bytes, falling back to the extension.
    ///
    /// Magic bytes come first because a `.txt` that is really a PDF would
    /// otherwise be handed to the AI as mojibake.
    pub fn detect(path: &Path, bytes: &[u8]) -> Result<Self> {
        if bytes.starts_with(b"%PDF-") {
            return Ok(Self::Pdf);
        }
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        // Android document providers often return a content URI without the
        // original filename. Inspect the archive instead of relying only on
        // the extension so a picked DOCX is still recognized.
        if bytes.starts_with(b"PK") {
            let is_docx = zip::ZipArchive::new(std::io::Cursor::new(bytes))
                .ok()
                .map(|mut archive| archive.by_name("word/document.xml").is_ok())
                .unwrap_or(false);
            return if is_docx {
                Ok(Self::Docx)
            } else {
                Err(CoreError::Invalid(
                    "that looks like a zip archive — use a PDF, DOCX or text file".into(),
                ))
            };
        }
        match extension.as_str() {
            "pdf" => Ok(Self::Pdf),
            "docx" => Ok(Self::Docx),
            "txt" | "md" | "text" | "" => Ok(Self::Text),
            other => Err(CoreError::Invalid(format!(
                "\"{other}\" files are not supported yet — use PDF, DOCX or a text file"
            ))),
        }
    }
}

/// Read a file, with a size guard so a stray huge file cannot exhaust memory.
pub fn read_checked(path: &Path) -> Result<Vec<u8>> {
    let meta = std::fs::metadata(path)?;
    if meta.len() > MAX_FILE_BYTES {
        return Err(CoreError::Invalid(format!(
            "that file is {} MB, which is larger than the {} MB limit",
            meta.len() / 1_048_576,
            MAX_FILE_BYTES / 1_048_576
        )));
    }
    if meta.len() == 0 {
        return Err(CoreError::Invalid("that file is empty".into()));
    }
    Ok(std::fs::read(path)?)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Detect the three image formats accepted for photo notes.
pub fn image_mime(bytes: &[u8]) -> Result<&'static str> {
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Ok("image/jpeg");
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Ok("image/png");
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Ok("image/webp");
    }
    Err(CoreError::Invalid(
        "each photo must be a JPEG, PNG or WebP image".into(),
    ))
}

/// Collapse the whitespace a PDF extractor leaves behind, while keeping the
/// blank lines that mark paragraphs — the chunker reads those.
pub fn tidy_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut blank_run = 0;

    for line in raw.lines() {
        let collapsed = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if collapsed.is_empty() {
            blank_run += 1;
            // One blank line marks a paragraph; more add nothing.
            if blank_run == 1 {
                out.push('\n');
            }
            continue;
        }
        blank_run = 0;
        // Rejoin a word split across lines: "loga-\nrithm" -> "logarithm".
        if out.ends_with("-\n") {
            out.truncate(out.len() - 2);
        }
        out.push_str(&collapsed);
        out.push('\n');
    }
    out.trim().to_string()
}

/// Pull the text out of a DOCX without a full OOXML parser.
///
/// A DOCX is a zip holding `word/document.xml`. Paragraph and break tags
/// become newlines, every other tag is dropped, and the handful of XML
/// entities Word emits are decoded.
fn extract_docx(bytes: &[u8]) -> Result<String> {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor)
        .map_err(|e| CoreError::Invalid(format!("that DOCX could not be opened: {e}")))?;
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .map_err(|_| CoreError::Invalid("that DOCX has no document body".into()))?
        .read_to_string(&mut xml)?;

    let mut out = String::with_capacity(xml.len() / 2);
    let mut in_tag = false;
    let mut tag = String::new();

    for ch in xml.chars() {
        match ch {
            '<' => {
                in_tag = true;
                tag.clear();
            }
            '>' => {
                in_tag = false;
                // </w:p> ends a paragraph; <w:br/> and <w:tab/> are breaks.
                if tag.starts_with("/w:p") || tag.starts_with("w:br") {
                    out.push('\n');
                } else if tag.starts_with("w:tab") {
                    out.push(' ');
                }
            }
            _ if in_tag => tag.push(ch),
            _ => out.push(ch),
        }
    }

    let decoded = out
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        // Ampersand last, so "&amp;lt;" does not become "<".
        .replace("&amp;", "&");
    Ok(tidy_text(&decoded))
}

fn extract_pdf(bytes: &[u8]) -> Result<String> {
    // pdf-extract panics on some malformed files rather than returning an
    // error, and a bad download should not take the window down with it.
    let parsed = std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem(bytes));
    match parsed {
        Ok(Ok(text)) => Ok(tidy_text(&text)),
        Ok(Err(e)) => Err(CoreError::Invalid(format!(
            "that PDF could not be read: {e}"
        ))),
        Err(_) => Err(CoreError::Invalid(
            "that PDF could not be read — it may be damaged".into(),
        )),
    }
}

/// Text extracted from one page, or from a document that has no pages.
#[derive(Debug, Clone, PartialEq)]
pub struct PageText {
    /// 1-based page number. `None` for DOCX and plain text, which have no
    /// pages, and for a PDF the extractor could not split.
    pub page: Option<u32>,
    pub text: String,
}

/// Pull text out page by page.
///
/// PDFs get real page numbers: `pdf-extract` has a per-page entry point, so the
/// count and order are the document's own. DOCX and plain text have no page
/// concept here — Word's layout lives in the printer's hands, and a text file
/// has no pages at all — so they come back as one page with no number, and
/// references show the material name only.
///
/// A PDF the per-page reader rejects falls back to the single-string extractor,
/// splitting on form feeds if it emitted any; failing that, one unnumbered page.
pub fn extract_pages(bytes: &[u8], kind: FileKind) -> Result<Vec<PageText>> {
    match kind {
        FileKind::Pdf => extract_pdf_pages(bytes),
        FileKind::Docx => Ok(vec![PageText {
            page: None,
            text: extract_docx(bytes)?,
        }]),
        FileKind::Text => Ok(vec![PageText {
            page: None,
            text: tidy_text(&String::from_utf8_lossy(bytes)),
        }]),
    }
}

fn extract_pdf_pages(bytes: &[u8]) -> Result<Vec<PageText>> {
    // Same panic guard as the single-string path: pdf-extract panics on some
    // malformed files, and a bad download must not take the window down.
    let parsed = std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem_by_pages(bytes));
    if let Ok(Ok(pages)) = parsed {
        // The walker numbers pages 1, 2, 3… and stops at the first page it
        // cannot read, which is why an empty result still falls through to the
        // whole-document reader below rather than being trusted.
        let mut out: Vec<PageText> = pages
            .into_iter()
            .enumerate()
            .map(|(index, text)| PageText {
                page: Some(index as u32 + 1),
                text: tidy_text(&text),
            })
            .collect();
        // A blank page is real, and dropping it must not renumber the pages
        // after it: the whole point of the number is the printed page.
        out.retain(|page| !page.text.trim().is_empty());
        if !out.is_empty() {
            return Ok(out);
        }
    }

    // Either the walker failed or every page came back blank. The
    // whole-document extractor is more forgiving, so try it before giving up —
    // and split on form feeds, which it emits between pages. The split has to
    // happen before tidying, because collapsing whitespace eats them, and a
    // document without form feeds keeps one unnumbered page: a guessed page
    // number would be a wrong citation.
    let raw = std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem(bytes));
    let raw = match raw {
        Ok(Ok(text)) => text,
        _ => {
            return Err(CoreError::Invalid(
                "that PDF could not be read — it may be damaged".into(),
            ))
        }
    };
    let pieces: Vec<&str> = raw.split('\u{0C}').collect();
    let out: Vec<PageText> = if pieces.len() > 1 {
        pieces
            .into_iter()
            .enumerate()
            .map(|(index, piece)| PageText {
                page: Some(index as u32 + 1),
                text: tidy_text(piece),
            })
            .filter(|page| !page.text.is_empty())
            .collect()
    } else {
        vec![PageText {
            page: None,
            text: tidy_text(&raw),
        }]
    };
    if out.is_empty() || out.iter().all(|page| page.text.trim().is_empty()) {
        return Err(CoreError::Invalid(NO_TEXT_IN_PDF.into()));
    }
    Ok(out)
}

/// Text extracted from one upload.
#[derive(Debug, Clone)]
pub struct Extracted {
    pub kind: FileKind,
    pub text: String,
    pub sha256: String,
    pub size: u64,
}

/// Extract readable text from bytes already in memory.
pub fn extract_from_bytes(path: &Path, bytes: &[u8]) -> Result<Extracted> {
    let kind = FileKind::detect(path, bytes)?;
    let text = match kind {
        FileKind::Pdf => extract_pdf(bytes)?,
        FileKind::Docx => extract_docx(bytes)?,
        FileKind::Text => tidy_text(&String::from_utf8_lossy(bytes)),
    };

    if text.trim().is_empty() {
        return Err(CoreError::Invalid(match kind {
            FileKind::Pdf => NO_TEXT_IN_PDF.to_string(),
            _ => "that file contains no readable text".to_string(),
        }));
    }

    Ok(Extracted {
        kind,
        sha256: sha256_hex(bytes),
        size: bytes.len() as u64,
        text,
    })
}

/// Read and extract in one step.
pub fn extract(path: &Path) -> Result<Extracted> {
    let bytes = read_checked(path)?;
    extract_from_bytes(path, &bytes)
}

// ---- cold storage -------------------------------------------------------

/// Compression level: 9 trades a little CPU at upload time for a smaller
/// folder, and these files are written once and almost never read.
#[cfg(target_os = "android")]
const ZSTD_LEVEL: i32 = 1;
#[cfg(not(target_os = "android"))]
const ZSTD_LEVEL: i32 = 9;

/// Format markers. `SBC1` is compressed only — written by builds before
/// encryption existed, still read so nobody loses an upload. `SBC2` is
/// compressed and then sealed with the device-derived file key.
const COLD_MAGIC_PLAIN: &[u8; 4] = b"SBC1";
const COLD_MAGIC_SEALED: &[u8; 4] = b"SBC2";

/// Where originals live, under the app data directory.
pub fn cold_dir(app_data: &Path) -> PathBuf {
    app_data.join("originals")
}

/// Reject anything that could climb out of the cold-storage directory. The
/// stored name comes from our own hash, but it reaches here via the database,
/// so it is treated as untrusted input.
fn safe_name(cold_path: &str) -> Result<&str> {
    if cold_path.is_empty()
        || cold_path.contains("..")
        || cold_path.contains('/')
        || cold_path.contains('\\')
        || cold_path.contains(':')
    {
        return Err(CoreError::Invalid(
            "that file reference is not valid".into(),
        ));
    }
    Ok(cold_path)
}

/// Store an original: compress, then encrypt with the device file key.
///
/// Returns the name to put in `materials.cold_path`. The folder it lands in
/// is useless on any other machine — the key is derived, never stored.
pub fn cold_store(app_data: &Path, key: &SecretKey, sha256: &str, bytes: &[u8]) -> Result<String> {
    let name = format!("{sha256}.sbc");
    safe_name(&name)?;

    let dir = cold_dir(app_data);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(&name);

    let compressed = zstd::encode_all(bytes, ZSTD_LEVEL)
        .map_err(|e| CoreError::Invalid(format!("could not compress the original: {e}")))?;

    // The magic is authenticated, so a sealed file cannot be relabelled as a
    // plaintext one to get the reader to skip decryption.
    let sealed = crypto::seal(key, COLD_MAGIC_SEALED, &compressed)?;

    // Write beside the target and rename, so a crash mid-write cannot leave a
    // half-written file that later fails to decrypt.
    let temp = dir.join(format!("{name}.part"));
    {
        let mut file = std::fs::File::create(&temp)?;
        file.write_all(COLD_MAGIC_SEALED)?;
        file.write_all(&sealed)?;
        file.sync_all()?;
    }
    std::fs::rename(&temp, &path)?;
    Ok(name)
}

/// Read an original back out, for "Download original".
pub fn cold_read(app_data: &Path, key: &SecretKey, cold_path: &str) -> Result<Vec<u8>> {
    let name = safe_name(cold_path)?;
    let bytes = std::fs::read(cold_dir(app_data).join(name))?;
    if bytes.len() < 4 {
        return Err(CoreError::Invalid(
            "the stored original is not in a format this version understands".into(),
        ));
    }

    let (magic, rest) = bytes.split_at(4);
    let compressed = match magic {
        m if m == COLD_MAGIC_SEALED => crypto::open(key, COLD_MAGIC_SEALED, rest)?,
        m if m == COLD_MAGIC_PLAIN => rest.to_vec(),
        _ => {
            return Err(CoreError::Invalid(
                "the stored original is not in a format this version understands".into(),
            ))
        }
    };

    zstd::decode_all(&compressed[..])
        .map_err(|e| CoreError::Invalid(format!("the stored original could not be read: {e}")))
}

/// Delete a stored original, e.g. when its material is removed.
pub fn cold_delete(app_data: &Path, cold_path: &str) -> Result<()> {
    let name = safe_name(cold_path)?;
    let path = cold_dir(app_data).join(name);
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}
