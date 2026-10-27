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

pub const PREVIEW_SYSTEM: &str = "You give a student a brief head start on a topic \
they have not been taught yet. Be conservative: cover only what is standard for the \
course, and say so through low confidence when you are unsure. Reply with JSON only.";

/// Ask for a small preview of the next topic (Brief 3.7).
///
/// The ceilings are stated in the prompt as well as enforced in Rust: a model
/// that writes less costs fewer tokens than one trimmed afterwards.
pub fn preview_user(subject: &str, recent: &[String], next_topic: Option<&str>) -> String {
    let covered = if recent.is_empty() {
        "nothing yet".to_string()
    } else {
        recent.join(", ")
    };
    let target = match next_topic {
        Some(topic) => format!("The next topic is \"{topic}\"."),
        None => "Work out what is usually taught next on this course.".to_string(),
    };

    format!(
        "Course: {subject}\nRecently covered: {covered}\n{target}\n\n\
         Give a short head start as JSON:\n\
         {{\"title\":string,\"keyIdeas\":[3-5 strings],\"cards\":[2-3 cards],\"confidence\":0.0-1.0}}\n\
         Cards use the same shapes as before (flash, mcq with 4 options, short).\n\
         Keep every idea to one sentence. This is a preview, not a lesson. \
         Set confidence below 0.5 if you are guessing at the course order."
    )
}

/// Marking a typed answer (Phase 5).
///
/// The model is told to mark meaning rather than wording, because the whole
/// point of letting the student type is that they answer in their own words.
/// It is also told to keep the model answer back when the student's wording
/// was already fine — being shown a "better" version of a correct answer every
/// single time trains people to stop reading the feedback.
pub const MARK_SYSTEM: &str = "You are a fair, experienced examiner marking a student's \
written answer. Mark what the answer MEANS, not the words it uses: a correct idea in the \
student's own words is correct. Be strict about missing content and lenient about phrasing. \
Never invent material that is not in the expected answer. Reply with JSON only.";

pub fn mark_user(question: &str, expected: &str, typed: &str) -> String {
    format!(
        "Question:\n{question}\n\n\
         Expected answer:\n{expected}\n\n\
         Student's answer:\n{typed}\n\n\
         Mark it out of 100 for content. Then reply with JSON only:\n\
         {{\"score\": 0-100, \"feedback\": \"...\", \"modelAnswer\": \"...\"}}\n\n\
         Rules:\n\
         - feedback: one or two sentences, speaking to the student. Say what was right and \
         what was missing or wrong. Do not repeat the whole answer back.\n\
         - modelAnswer: how to write this answer so it would earn full marks in an exam. \
         Keep the student's own understanding, fix only what would cost marks.\n\
         - If the answer is correct AND already well expressed, set modelAnswer to \"\".\n\
         - Judge only against the expected answer above."
    )
}
