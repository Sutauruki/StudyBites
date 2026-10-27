# StudyBites — Project Brief for Claude Code

> Working name: **StudyBites** (rename later if needed).
> Owner: Sutauruki (GitHub: `sutauruki`). Solo developer, university student, Lagos, Nigeria.
> This file is the single source of truth. Read it fully before writing any code.

---

## 0. Critical constraints (read first)

1. **No building on the owner's laptop.** The owner will NOT install Rust, Visual Studio Build Tools, or any heavy toolchain locally (data and disk limits). All coding happens in this cloud session / GitHub Codespaces. **The Windows installer must be produced by GitHub Actions** on a `windows-latest` runner. The owner downloads only the finished `.exe` installer to test.
2. **Target machine is low-end.** HP EliteBook 850 G1: 4th-gen Intel Core i5/i7 (Haswell, dual-core), **12 GB RAM**, integrated Intel HD graphics, Windows. Everything must feel fast on this.
3. **Private repo, auto-builds on merge.** The owner should not need Codespaces just to get a build. Every push/merge to `main` automatically builds the Windows installer and publishes it to a rolling **"latest" pre-release**, so the owner just downloads the `.exe` from the Releases page. Because Actions minutes are limited (Windows runners use them faster), build **only on `main`** (not feature branches), skip docs-only changes, and cancel superseded runs (Section 11).
4. **Minimal dependencies.** Every crate/npm package must justify itself. Small installer, low RAM.
5. **Work in phases** (Section 12). Commit at the end of each phase. Don't start a phase until the previous one builds in CI.

### Performance budgets (on the target machine)
- Installer: **≤ 40 MB** (embedding model included), app shell itself ideally ≤ 15 MB
- Idle RAM: **≤ 200 MB**
- Cold start to usable screen: **≤ 2 s**
- Any study-session interaction (flip card, grade, next): **< 100 ms**, no network needed
- Heavy work (text extraction, embedding, AI calls) always runs **in the background** with visible progress; the UI never freezes

---

## 1. What the app is

A **distraction-free desktop study app** that turns a student's own class materials into short, low-stress study sessions ("bites" of 5–15 minutes), and remembers progress day to day using spaced repetition.

Core daily loop:
1. Student comes back from class.
2. Uploads that day's notes/PDFs for a subject.
3. App distills them into compact notes + flashcards + questions, tagged to a topic.
4. Student does a short session before sleeping.
5. Next day, the app knows what was studied and brings back what's due, plus new material.
6. Optionally, the app offers a small **preview of the next topic** so the student isn't lost in the next class.

Primary user: the owner (for his own studies). Secondary: fellow students who buy it, delivered in person with device-locked activation.

**The app ships with no built-in content or memory.** Every install starts empty. The student creates their own subjects and uploads their own materials.

---

## 2. Tech stack

| Layer | Choice | Notes |
|---|---|---|
| Desktop shell | **Tauri v2** | Uses system WebView2 (already on Win10/11). Android is possible later from the same codebase. |
| Frontend | **React + TypeScript + Vite** | Plain CSS or CSS modules with design tokens. No heavy UI kits. Small state library (Zustand) only if needed. |
| Backend | **Rust** (Tauri commands) | All processing, crypto, DB and AI calls live in Rust. The frontend never holds secrets. |
| Database | **SQLite via rusqlite with SQLCipher** (`bundled-sqlcipher` feature) | Entire DB encrypted at rest. |
| Vector search | **Brute-force cosine similarity in Rust** over embeddings stored as BLOBs | One student's corpus is a few thousand chunks, so brute force is fast and avoids extension-loading problems with SQLCipher. Use `sqlite-vec` only if it links cleanly with SQLCipher and is clearly worth it. |
| Embeddings | **Local ONNX model**: `all-MiniLM-L6-v2` (quantized, about 23 MB) via `ort` or `fastembed-rs` | Runs on CPU, free, offline. Bundle it in the installer so the user downloads nothing extra. |
| PDF text | `pdf-extract` or `lopdf`-based extraction in Rust | Images and scanned pages: send to the AI provider's vision model when online (Phase 2+). |
| DOCX/TXT | Lightweight Rust parsing (`docx-rs` or a zip+xml read) | |
| Spaced repetition | **FSRS** via the `fsrs` Rust crate (or a port of ts-fsrs logic) | |
| AI (generation) | Provider abstraction: **Gemini (free tier) primary**, Groq and OpenRouter free models as fallbacks | Online-first. Local Ollama is a later phase. |
| Web search (previews) | Gemini Search grounding, or a Tavily/Brave free tier as fallback | Used sparingly (Section 7). |
| Crypto | RustCrypto / `ed25519-dalek`, `chacha20poly1305` or `aes-gcm`, `argon2`, `hkdf`, `sha2` | **Never homemade crypto.** |
| Secrets storage | Windows DPAPI / OS keychain via the `keyring` crate | For the user's API key. |

---

## 3. User flow & features

### 3.1 Subjects
- Create, rename and archive subjects (e.g. "MTH 101", "Intro to Biology").
- Each subject has an optional **scheme of work** and a list of materials, topics and cards.

### 3.2 Scheme of work (OPTIONAL)
- The student can upload a scheme of work (PDF/DOCX/text/photo) or skip it.
- If uploaded, the AI converts it into an **ordered topic outline**: topics → subtopics, with week numbers if present. Show it to the student so they can edit, reorder, add or delete topics before saving.
- If skipped, topics are created by the student as they upload, and the outline grows over time.

### 3.3 Uploading class material and topic tagging
- Accept PDF, DOCX, TXT, pasted text, and images/photos of notes (images in Phase 2+).
- After upload, **always ask which topic this belongs to.** Don't guess silently.
  - If a scheme exists: the AI reads the material, compares it to the outline and **pre-selects the most likely topic**. The student taps **Confirm** or picks another topic from the list.
  - Always offer **"Something else"**. The student types a short description (e.g. "Lecturer did a revision on logarithms"). This creates an **off-scheme topic**, and the AI suggests which scheme topic it supports (the student can accept or skip that link).
  - If no scheme: the student picks an existing topic or names a new one.
- The confirmed topic is attached to every chunk, note and card produced from that upload.

### 3.4 Ingestion pipeline (Rust, background job, resumable)
1. **Extract** text from the file.
2. **Chunk** it by structure (headings and paragraphs), roughly 300–600 tokens per chunk with a small overlap.
3. **Distill** each chunk with the AI into a compact study note as strict JSON: key ideas, definitions, formulas, worked examples, and common mistakes. Keep it short and faithful to the source.
4. **Generate cards** from each distilled note: flashcards (front/back), MCQs (4 options, one correct, short explanation), and short-answer prompts. Ask for structured JSON output and validate it in Rust. On invalid JSON, retry once, then skip and log.
5. **Embed** each distilled note locally and store the vector.
6. **Store the original file compressed and encrypted** in a cold-storage folder (Section 9). It is **never loaded during normal use**. It exists only for the "Download original" feature and for re-processing later with a better model.
7. **Never keep raw extracted text in the hot path.** The app works only with distilled notes, cards and embeddings.

Jobs are queued and persisted. If the network drops or the daily AI quota is hit, the job pauses and resumes later (Section 7.3). Show per-upload status: Queued / Processing x of y / Done / Paused (quota) / Failed.

### 3.5 Study sessions ("bites")
- The home screen shows one primary action: **"Start a bite"**, with a duration choice (5 / 10 / 15 min).
- Session mix: due FSRS reviews first, then a few new cards from the most recent material. Cap new cards per session (default 10) so it never feels heavy.
- Card types: flashcard (reveal, then grade Again/Hard/Good/Easy), MCQ (instant feedback with the explanation), short answer (reveal the model answer, then self-grade).
- **When time is up, the session ends** with a calm summary (cards done, topics touched, what's due tomorrow). No guilt, no streak shaming, and no endless "one more" loops.
- Grading updates FSRS state immediately in the DB.

### 3.6 Day-to-day memory
- "Remembering yesterday" comes from **database state, not AI memory**: FSRS card states, review logs, session history, and per-topic mastery. Zero tokens, instant to load.
- Per-topic mastery is shown as a simple bar per topic in the subject view.

### 3.7 "Prepare for next class" previews
- After a session (or from the subject view), the app may offer: *"Next is probably **X**. Want a 5-minute head start?"* It is **offered, never forced**, and can be dismissed.
- Working out "next":
  - **With a scheme:** the next uncovered topic in the outline. No web search needed.
  - **Without a scheme, or for subtopics inside a topic:** the AI infers the usual teaching order from the course and recent topics. Use web search **only if the model reports low confidence**, to save quota.
- A preview is **small**: 3–5 key ideas plus 2–3 cards. Nothing more.
- Previews are **clearly labeled "AI preview — not from your class"** everywhere they appear (badge on the cards and the topic).
- When real class material for that topic is uploaded later, the real notes take priority. Preview cards are retired, or merged where they duplicate real content. Preview cards never outrank real cards in scheduling.

### 3.8 Ask / Explain (lightweight RAG)
- On any card, an "Explain" button: retrieve the top 3–5 most similar distilled notes from **that subject only** and ask the AI for a short explanation grounded in them. It is cheap in tokens by design.
- Optional subject-level "Ask a question" box with the same retrieval, showing which notes it used.

### 3.9 Exam mode (Phase 4)
- Set an exam date per subject. As the date gets closer, the session mix shifts toward weak topics and full coverage, still within the bite-size limits.

### 3.10 Download original
- From a material's detail page: decrypt and decompress the original file and save it to a location the user chooses.

---

## 4. UI / UX direction

- **Minimal and distraction-free.** One main thing per screen. Lots of whitespace, a calm palette, and large readable type for cards.
- Light and dark themes; follow the system by default.
- Full keyboard support during sessions: Space = reveal, 1–4 = grade, Esc = end session.
- No notifications spam, no gamification clutter, no badges or streak pressure. At most a gentle "X cards due today".
- Clear, friendly empty states that guide setup (first run: create a subject → optional scheme → first upload).
- Use CSS transitions sparingly. Avoid animation libraries.
- The window stays responsive at all times; long tasks show a slim progress indicator.
- Settings: API key(s), daily new-card limit, default bite length, theme, and license info.

---

## 5. Data model (initial)

Every table lives in the SQLCipher-encrypted DB.

- `subjects(id, name, created_at, archived)`
- `scheme_topics(id, subject_id, parent_id NULL, title, week NULL, position, is_off_scheme BOOL, supports_topic_id NULL, is_preview BOOL)`
- `materials(id, subject_id, topic_id, original_filename, mime, size, sha256, cold_path, uploaded_at, status, error NULL)`
- `notes(id, material_id NULL, topic_id, content_json, embedding BLOB, is_preview BOOL, created_at)`
- `cards(id, note_id, topic_id, kind ENUM(flash, mcq, short), payload_json, is_preview BOOL, retired BOOL, created_at)`
- `card_state(card_id PK, fsrs fields: stability, difficulty, due, last_review, reps, lapses, state)`
- `review_log(id, card_id, rating, reviewed_at, elapsed_ms)`
- `sessions(id, subject_id NULL, started_at, ended_at, planned_minutes, cards_done)`
- `jobs(id, kind, payload_json, status, attempts, next_run_at, last_error)`
- `quota_usage(provider, date, requests, tokens)`
- `settings(key, value)`

Use versioned migrations from day one.

---

## 6. Token efficiency rules

- Distill **once** per chunk at upload time. Never re-send raw material to the AI during study.
- Retrieval always sends only the **top-k distilled notes** (k = 3–5), never whole documents.
- Use structured JSON output with tight schemas and short system prompts.
- Batch several small chunks into one request where the provider allows it, to save request-count quotas.
- Cache AI responses by content hash, so re-uploading the same file costs nothing.

---

## 7. AI provider layer

### 7.1 Abstraction
A Rust trait `AiProvider` with methods such as `distill`, `generate_cards`, `match_topic`, `parse_scheme`, `explain`, and `preview_next(with_search: bool)`. Implementations: `Gemini`, `Groq`, `OpenRouter`. Later: `Ollama` (local, offline). A router picks the provider in a user-set priority order and falls back on errors or quota limits.

### 7.2 API keys
- **Each user enters their own free API key(s)** in Settings. The owner's key is **never** embedded in the app, because keys inside a binary can be extracted.
- Keys are stored in the OS keychain/DPAPI, not in the DB or any plain file.
- The settings screen includes a short "How to get a free Gemini key" help text.

### 7.3 Daily quota handling
- Track requests and tokens per provider per day in `quota_usage`.
- When a provider returns a rate-limit or quota error, mark it exhausted until its reset time and fall back to the next provider. If all are exhausted, pause the job queue and show: "Daily AI limit reached — processing resumes automatically after reset." Studying keeps working fully offline.
- Exponential backoff on transient errors.

### 7.4 Later: offline mode
- Optional `Ollama` provider for fully local generation. A 3B model is comfortable on 12 GB RAM; 7B Q4 is possible but slow. Generation runs in the background or overnight. Not part of the first phases.

---

## 8. Licensing & anti-sharing

Goal: an installed copy works **only on the device it was activated for**. Copying the installer, the app folder or the data to another PC or phone gets the other person nothing.

### 8.1 Device fingerprint
- Combine stable hardware/OS identifiers: Windows `MachineGuid`, system disk serial, CPU identifier, motherboard/BIOS serial (via WMI or registry reads in Rust).
- Normalize, then hash with SHA-256 and a fixed app salt to get `device_id`.
- Tolerate one component changing (e.g. a swapped disk) by storing per-component hashes and accepting a match on most components. Document the threshold.

### 8.2 Activation flow (offline, in person)
1. On first launch, the app shows an **activation screen** with a **Request Code** (a short, human-typeable encoding of `device_id` plus an app version/nonce).
2. The owner enters that code into a **separate admin tool** (Section 8.4), which outputs an **Activation Code**: an Ed25519-signed payload `{device_id, license_id, buyer_name, issued_at, features}`.
3. The student enters the Activation Code. The app verifies the signature with the **embedded public key** and checks that `device_id` matches. If valid, the license is stored and the app unlocks.
4. No internet is needed for activation.

### 8.3 Reinstall policy (decided)
- **Reinstalling on the same device works** with the same activation code. That's harmless, because the license only works on that device.
- **We do NOT try to block reinstalls.** It isn't reliably possible (an OS reset wipes any trace) and it would punish honest users whose laptops crash.

### 8.4 Admin license tool (separate crate in the repo)
- A small CLI, `license-tool`, with commands to generate a keypair (once), issue an activation code from a request code plus buyer name, and keep a local list of issued licenses.
- **The Ed25519 private key must NEVER be committed to the repo or embedded in the app.** It stays on the owner's machine (generated by the tool, path given by the owner, and gitignored). Only the public key goes into the app source.
- The tool should also be buildable as a tiny Windows `.exe` artifact in CI so the owner doesn't need Rust.

### 8.5 Honest limitation (documented in the README)
No software running on a machine the user controls is unbreakable. A skilled attacker could patch the binary. The aim is to make copying useless and cracking more effort than it's worth.

---

## 9. Encryption at rest

- **Database:** SQLCipher. The DB key is derived with `HKDF-SHA256(ikm = device_id || license_secret_from_activation, salt = app_salt, info = "db-key")`. A DB file copied to another machine cannot be opened.
- **Cold-storage originals:** zstd-compressed, then encrypted with **XChaCha20-Poly1305** (or AES-256-GCM) using a key derived the same way (`info = "file-key"`) and a random nonce per file. Stored under the app data directory with random filenames.
- **API keys:** OS keychain/DPAPI only.
- **No plaintext temp files.** Extraction works in memory where possible; if a temp file is unavoidable, delete it right after use.
- Zeroize key material in memory after use (`zeroize` crate).

---

## 10. App hardening (release builds)

- Release profile: `lto = true`, `codegen-units = 1`, `opt-level = "s"` or `3` (measure both), `strip = true`, `panic = "abort"`.
- **Devtools disabled** in release builds.
- **Strict CSP.** The frontend can talk only to Tauri IPC. All network calls go through Rust, never `fetch` from the WebView.
- Tauri v2 **capabilities**: allow only the exact commands and plugins needed. No shell, no broad filesystem access from the frontend. Use the dialog plugin for file pickers.
- Validate every IPC input in Rust.
- Basic integrity check at startup (e.g. verify the license blob signature on every launch and refuse to run if the license is invalid).
- No telemetry, analytics or crash reporting that sends data anywhere.

---

## 11. Build & delivery (CI)

### 11.1 Workflow
- File: `.github/workflows/build-windows.yml`
- Triggers:
  - `push` to **`main`** (this covers merged PRs too), with `paths-ignore` for docs-only changes (`**.md`, `docs/**`).
  - `workflow_dispatch` (manual run, as a backup).
  - Tags `v*` for proper versioned releases.
- `concurrency: { group: build-windows-${{ github.ref }}, cancel-in-progress: true }` so a newer push cancels an older running build and saves minutes.
- Runner: `windows-latest`.
- Uses `tauri-apps/tauri-action` (or the equivalent manual steps) to build the **NSIS `.exe` installer**.
- Rust and npm caching (`Swatinem/rust-cache`, npm cache) to keep runs short.
- Publishing:
  - On every `main` build: replace the assets of a rolling pre-release tagged **`latest`** (titled "Latest build", with the commit SHA and date in its notes). The owner always finds the newest `.exe` in the same place.
  - Also upload the installer as a workflow **artifact** (short retention, e.g. 7 days, to save storage).
  - On `v*` tags: create a normal versioned GitHub Release with the installer attached.
- The workflow needs `permissions: contents: write` to manage releases.
- Feature branches/PRs: a cheap Linux job only (`cargo check`, `cargo test`, frontend typecheck/build). **No Windows build** until merged to `main`.
- A second job (or the same job) builds `license-tool.exe` and uploads it as a separate artifact.
- The ONNX embedding model is downloaded during CI (or fetched via Git LFS) and bundled as a Tauri resource. Never commit large binaries without LFS.

### 11.2 How the owner gets the executable (put this in the README)
No Codespace needed for this.
1. Claude Code pushes/merges to `main`. The build starts automatically.
2. Wait for the green check on the commit (GitHub → repo → **Actions**, about 10–20 min on the first build, faster with cache).
3. Go to GitHub → repo → **Releases** → **"Latest build"** and download the `.exe`. Or download the artifact from the Actions run page.
4. On the EliteBook: run the `.exe` installer. Because the app isn't code-signed, Windows SmartScreen will warn: click **More info → Run anyway**.

### 11.3 Local dev in Codespaces (no desktop window)
- `npm run dev` runs the React UI in the browser via a forwarded port, with a **mock backend layer** (a TS module that fakes the Tauri commands) so UI work doesn't need the native shell.
- `cargo test` covers all Rust logic: chunking, FSRS scheduling, crypto round-trips, license verification, provider JSON parsing, and the quota router.
- Provide a `.devcontainer/devcontainer.json` with Rust, Node, and the Tauri Linux deps so `cargo check` and the tests work in Codespaces.

---

## 12. Phases & acceptance criteria

**Phase 1 — Shell & foundations**
- Tauri v2 + React/TS scaffold, design tokens, light/dark themes, base layout.
- SQLCipher DB with migrations (temporary fixed dev key until Phase 3).
- Subjects CRUD; optional scheme-of-work entry (manual outline editor first).
- Mock backend for browser dev; devcontainer; Windows CI workflow producing an installer.
- ✅ Done when: the owner downloads the CI installer, installs it on the EliteBook, creates subjects and topics, and the app stays within the performance budgets.

**Phase 2 — Ingestion, AI & studying**
- Provider layer (Gemini first, then Groq/OpenRouter fallback), keychain storage for keys, quota tracking and job queue.
- AI scheme parsing; upload → topic match/confirm → extract → chunk → distill → cards → local embeddings; cold storage (encryption can be stubbed until Phase 3).
- FSRS bite sessions, session summary, per-topic mastery, Explain (RAG).
- ✅ Done when: the owner uploads a real lecture PDF, confirms the topic, gets cards, studies a 10-minute bite, and the next day sees the correct due reviews.

**Phase 3 — Security & licensing**
- Device fingerprint, activation screen, Ed25519 verification, `license-tool` crate plus its CI artifact.
- Device-derived keys for SQLCipher and cold-storage encryption; zeroize; release hardening; strict CSP and capabilities.
- ✅ Done when: an activated install works; copying the app data folder to another PC fails to open; a forged or altered activation code is rejected; reinstalling on the same device with the same code works.

**Phase 4 — Smart extras**
- "Prepare for next class" previews (labeled, small, optional, with merge/retire logic), web-search fallback, exam mode, and Download original.
- ✅ Done when: after studying topic N, the app offers a labeled preview of topic N+1, and uploading real notes for N+1 later retires the preview cards.

**Phase 5 — Answering and practising** (added after Phase 3; see docs/DECISIONS.md D31-D33)
- Short-answer cards are written out and marked against the real answer, with feedback and, where the wording would cost marks, how to write it for full marks. Marked locally where possible, by AI only for genuine judgement calls, and falling back to self-grading whenever no provider can be reached. Per subject, on by default.
- CBT practice tests: pick subjects, a question count and a duration; a timed multiple-choice mock drawn from the student's own cards, with a numbered grid, flagging, auto-submit and a review of what was missed. Kept entirely apart from the spaced-repetition schedule.
- ✅ Done when: a typed answer in the student's own words is marked correct, a test can be sat end to end offline, and sitting one leaves every card's due date unchanged.

**Later (not now):** local Ollama offline mode, Android build from the same codebase, OCR for scanned PDFs without AI.

---

## 13. Licensing of the code & EULA

- This is **proprietary, closed-source software**. Do **not** add any open-source license (no MIT, GPL, Apache, etc.), and do not add license badges suggesting otherwise.
- Create a `LICENSE` file at the repo root (new file) with exactly:

```
Copyright (c) 2026 Odejayi Ezekiel (Sutauruki). All rights reserved.

This software and its source code are proprietary and confidential.
No part of it may be copied, modified, distributed, or used without
written permission from the copyright holder.
```

- In `package.json` set `"license": "UNLICENSED"` and `"private": true`. In every `Cargo.toml` set `publish = false` and `license-file = "LICENSE"` (relative path adjusted per crate).
- **EULA (Phase 3):** add an end-user license agreement screen shown at first launch, before activation. The user must accept it to continue, and acceptance (version + timestamp) is stored in the DB. Store the text in `src-tauri/resources/EULA.md` so the owner can edit it. Draft content (owner will refine; not legal advice):
  - The software is licensed, not sold, to the named buyer for use on **one activated device**.
  - No copying, sharing, reselling, renting or transferring the software or activation code.
  - No reverse engineering, decompiling or bypassing the licensing/encryption, except where law explicitly allows it.
  - The user's study materials remain theirs, stored only on their device. The app sends content only to the AI provider(s) the user configures with their own API key.
  - AI-generated notes, cards and previews may contain mistakes; the user should verify them against official course material.
  - The software is provided "as is", without warranty; liability is limited to the amount paid.
  - The license ends automatically if these terms are broken.
- Also show the EULA (read-only) under Settings → About.

---

## 14. Working rules for Claude Code

- Follow the phases in order. At the end of each phase: make sure `cargo test` passes, CI builds green, update the README, and commit with a clear message.
- When you deliver or change files, always state the **exact file path** and whether it's a **new file or a replacement**.
- Prefer simple, well-maintained crates. If a dependency would add a lot of installer size or RAM, note it and pick a lighter option.
- Never commit secrets, private keys or API keys. Add a `.gitignore` covering them.
- If something in this brief is technically impossible or clearly worse than an alternative, say so briefly, propose the alternative, and continue with the best option rather than stalling.
- Keep a `docs/DECISIONS.md` log of notable technical decisions.
- At the end of every session, tell the owner exactly which commands to run (in a Codespace or the GitHub UI) to get the latest installer.
