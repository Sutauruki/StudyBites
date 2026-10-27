//! Prompt text (Brief 6).
//!
//! Every prompt here is short on purpose. Free tiers are counted in requests
//! and tokens, and a long preamble is paid for on every single chunk of every
//! upload. The schema is stated once, tersely, and the model is told to return
//! JSON only — `ai::extract_json` copes with the cases where it does not.

use crate::ai::DistilledNote;

/// Keep retrieved notes from crowding out the question in a small context.
const MAX_NOTE_CHARS: usize = 1_200;

pub const DISTILL_SYSTEM: &str = "You compress lecture material into study notes. \
Be faithful to the source: never add facts it does not contain. \
Reply with JSON only, no prose, no code fences.";

/// Ask for one compact note from one chunk.
pub fn distill_user(
    subject: &str,
    topic: Option<&str>,
    heading: Option<&str>,
    text: &str,
) -> String {
    let mut prompt = format!("Subject: {subject}\n");
    if let Some(topic) = topic {
        prompt.push_str(&format!("Topic: {topic}\n"));
    }
    if let Some(heading) = heading {
        prompt.push_str(&format!("Section: {heading}\n"));
    }
    prompt.push_str(
        "\nSummarise the material below as JSON of this exact shape:\n\
         {\"title\":string,\"keyIdeas\":[string],\"definitions\":[{\"term\":string,\"meaning\":string}],\
         \"formulas\":[string],\"workedExamples\":[string],\"commonMistakes\":[string]}\n\
         Keep every entry to one or two sentences. Use [] for anything the \
         material does not cover. Do not invent examples.\n\n---\n",
    );
    prompt.push_str(text);
    prompt
}

pub const CARDS_SYSTEM: &str = "You write spaced-repetition cards from study notes. \
Every card must be answerable from the notes alone. \
Reply with JSON only, no prose, no code fences.";

/// Ask for a small mixed set of cards from one note.
pub fn cards_user(note: &DistilledNote) -> String {
    let notes_json = serde_json::to_string(note).unwrap_or_default();
    format!(
        "From these notes write between 3 and 6 cards as a JSON array.\n\
         Each element is one of:\n\
         {{\"kind\":\"flash\",\"front\":string,\"back\":string}}\n\
         {{\"kind\":\"mcq\",\"question\":string,\"options\":[4 strings],\"answer\":0-3,\"explanation\":string}}\n\
         {{\"kind\":\"short\",\"prompt\":string,\"answer\":string}}\n\
         Rules: MCQs need exactly 4 distinct options with one correct answer, \
         and `answer` is its index. Prefer understanding over recall of \
         wording. Keep answers short.\n\n{notes_json}"
    )
}

pub const TOPIC_MATCH_SYSTEM: &str =
    "You match lecture material to a course outline. Reply with JSON only.";

/// Ask which outline topic a piece of material belongs to (Brief 3.3).
///
/// The model is told it may decline: a confident wrong guess is worse than
/// "not sure", because the student confirms the choice either way.
pub fn topic_match_user(text: &str, topics: &[String]) -> String {
    let list = topics
        .iter()
        .enumerate()
        .map(|(i, t)| format!("{i}: {t}"))
        .collect::<Vec<_>>()
        .join("\n");

    let excerpt: String = text.chars().take(2_000).collect();
    format!(
        "Course outline:\n{list}\n\n\
         Which outline entry does this material belong to? Reply as\n\
         {{\"index\":number or null,\"confidence\":0.0-1.0,\"reason\":string}}\n\
         Use null when none of them fit. Be honest about low confidence.\n\n\
         ---\n{excerpt}"
    )
}

pub const SCHEME_SYSTEM: &str =
    "You convert a scheme of work into an ordered outline. Reply with JSON only.";

/// Ask for a scheme of work as a structured outline (Brief 3.2).
pub fn scheme_user(text: &str) -> String {
    let excerpt: String = text.chars().take(12_000).collect();
    format!(
        "Convert this scheme of work into a JSON array, in teaching order:\n\
         [{{\"title\":string,\"week\":number or null,\"subtopics\":[string]}}]\n\
         Keep the wording of the document. Include a week number only when the \
         document gives one. Do not invent topics.\n\n---\n{excerpt}"
    )
}

pub const EXPLAIN_SYSTEM: &str = "You are a patient tutor. Explain using only the \
student's own notes provided. If they do not cover the question, say so plainly \
rather than guessing.";

/// Ask for a short explanation grounded in retrieved notes (Brief 3.8).
pub fn explain_user(question: &str, notes: &[DistilledNote]) -> String {
    let context = notes
        .iter()
        .enumerate()
        .map(|(i, note)| {
            let body: String = note.as_plain_text().chars().take(MAX_NOTE_CHARS).collect();
            format!("[{}] {}", i + 1, body)
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    format!(
        "Notes from the student's own material:\n\n{context}\n\n---\n\
         Question: {question}\n\n\
         Answer in at most 120 words, in plain language. Cite the notes you \
         used as [1], [2]. If the notes do not answer it, say which part is \
         missing."
    )
}
