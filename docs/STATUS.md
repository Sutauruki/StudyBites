## In progress

Task: page references (roadmap item 6). Branch `feature/page-references`.
Done: nothing yet — starting Part A (housekeeping).
Next: Part A docs, then B1–B6, then tests, then the PR.
Blockers: none. Rust checks run only in CI on this laptop.

=== STUDYBITES HANDOFF REPORT ===
Task: C — Android app-state recovery | Status: done
Branch and last commit: ccr-cd109432-nnvymz 2ce8193 | CI: not run
1. What I did (one line per change):
 - crates/studybites-core/tests/storage.rs (edited): added an upgrade test applying the resumable-session migration to a populated pre-existing database.
 - src/App.tsx (edited): clear the in-memory resume snapshot when a study session finishes; this pre-existing workspace edit was included with Task C.
 - Existing Task C commits already contain the session_cards migration, Rust resume/stale-session logic, command and mock support, versioned local snapshot, README, D38, and roadmap update.
2. Checks I ran (command and result):
 - cargo test -p studybites-core: PASSED (all suites; 100 tests)
 - npm run typecheck: PASSED
 - npm run build:web: PASSED
 - git diff --check: PASSED
 - Failures: none
3. Not verified:
 - Android device checklist: background to WhatsApp, process swipe-away/reopen, and Developer options "Don't keep activities".
4. Where I changed or went beyond the prompt, and why:
 - Added an explicit populated-database migration test to cover the stated upgrade scenario.
5. Problems, risks and anything I guessed:
 - No frontend test runner is configured; Rust integration, typecheck, and production build checks passed.
6. What the developer must do or test now (device checklist, secrets, settings):
 - Verify on Android: start a 10-minute session and grade two cards; switch to WhatsApp and return; swipe away and reopen; repeat with "Don't keep activities". Confirm the same screen/card and timer are restored.
7. What is left:
 - Remaining parts of this task: Android device verification
 - Roadmap status: item 1 [~], item 11 [~]; all other items 2-10 and 12-16 are [ ]
 - Suggested next step: continue to Task D; retain the device checklist for validation.
READY FOR NEXT: yes
=== END REPORT ===

=== STUDYBITES HANDOFF REPORT ===
Task: D — Photo upload | Status: partly done
Branch and last commit: ccr-cd109432-nnvymz b03571f | CI: not run
1. What I did (one line per change):
 - Edited: Cargo.lock, README.md, crates/studybites-core/Cargo.toml, crates/studybites-core/src/ai.rs, crates/studybites-core/src/files.rs, crates/studybites-core/src/ingest.rs, crates/studybites-core/src/jobs.rs, crates/studybites-core/src/library.rs, crates/studybites-core/src/migrations.rs, crates/studybites-core/src/prompts.rs, crates/studybites-core/src/providers.rs, crates/studybites-core/tests/pipeline.rs, crates/studybites-core/tests/storage.rs, crates/studybites-core/tests/study_flow.rs, docs/DECISIONS.md, docs/ROADMAP.md, src-tauri/Cargo.toml, src-tauri/src/lib.rs, src-tauri/src/phase2.rs, src-tauri/src/worker.rs, src/api/mock.ts, src/api/tauri.ts, src/api/types.ts, src/routes/HomePage.tsx, src/routes/SubjectPage.tsx, src/routes/UploadDialog.tsx, src/styles/app.css: added encrypted per-page photo storage, background transcription, student review/confirmation and student-notes distillation; updated mocks, documentation, and tests.
 - New: src/lib/image.ts: resize photos locally to JPEG.
 - New: src/routes/PhotoReviewDialog.tsx: edit transcripts and mark diagram rectangles.
 - Roadmap item 2 remains [~] pending device verification.
2. Checks I ran (command and result):
 - cargo test -p studybites-core: PASSED
 - cargo check --workspace --all-targets: PASSED
 - cargo clippy --workspace --all-targets -- -D warnings: PASSED
 - cargo fmt --all --check: PASSED
 - npm run typecheck; npm run build:web: PASSED
 - git diff --check: PASSED
 - Frontend tests: NOT RUN (no test script or runner is configured).
 - Failures: none.
3. Not verified:
 - Acceptance checklist: (1) photo picker, thumbnails and reorder; (2) offline upload/waiting UI; (3) live Gemini transcription; (4) highlighted/editable [?] words; (5) notes/cards and labels on device; (6) no-image-provider pause UI; (7) duplicate upload behavior on device; (8) existing PDF/DOCX UI flow on device; (9) Needs review persistence across leaving/reopening. Core tests cover duplicate, provider pause, and existing DOCX extraction; device behavior remains not verified.
4. Where I changed or went beyond the prompt, and why:
 - Added material_page_image_get so review can display the encrypted page; deletion also preserves cold files still referenced elsewhere. Design recorded in D39.
5. Problems, risks and anything I guessed:
 - Live provider requests and Android picker behavior were not tested; no real API key was used.
6. What the developer must do or test now (device checklist, secrets, settings):
 - On Android: pick 3 photos, reorder, upload offline, then connect a Gemini key and verify transcription, editing/diagram marking, confirmed notes/cards, clear no-vision pause, duplicate re-upload, PDF/DOCX upload, and Needs review persistence after leaving/reopening.
7. What is left:
 - Remaining parts of this task: nine-item device acceptance checklist above.
 - Roadmap status: item 1 [~], item 2 [~], item 11 [~]; all other items [ ].
 - Suggested next step: continue to Task E; return to the device checklist before marking item 2 complete.
READY FOR NEXT: yes
=== END REPORT ===

=== STUDYBITES HANDOFF REPORT ===
Task: E — Android Do Not Disturb | Status: partly done
Branch and last commit: ccr-cd109432-nnvymz 272d441 | CI: not run
1. What I did (one line per change):
 - Edited: Cargo.lock, Cargo.toml, README.md, docs/DECISIONS.md (D40), docs/ROADMAP.md, src-tauri/Cargo.toml, src-tauri/capabilities/default.json, src-tauri/src/lib.rs, src/App.tsx, src/api/mock.ts, src/api/tauri.ts, src/api/types.ts, src/routes/SettingsPage.tsx, src/routes/StudyPage.tsx: integrated Android-only DND, Settings controls, session lifecycle restoration, startup recovery, and documentation.
 - New: src-tauri/plugins/tauri-plugin-dnd/.gitignore, src-tauri/plugins/tauri-plugin-dnd/Cargo.toml, src-tauri/plugins/tauri-plugin-dnd/README.md, src-tauri/plugins/tauri-plugin-dnd/build.rs, src-tauri/plugins/tauri-plugin-dnd/android/.gitignore, src-tauri/plugins/tauri-plugin-dnd/android/build.gradle.kts, src-tauri/plugins/tauri-plugin-dnd/android/consumer-rules.pro, src-tauri/plugins/tauri-plugin-dnd/android/settings.gradle, src-tauri/plugins/tauri-plugin-dnd/android/src/main/AndroidManifest.xml, src-tauri/plugins/tauri-plugin-dnd/android/src/main/java/com/sutauruki/studybites/dnd/DndPlugin.kt, src-tauri/plugins/tauri-plugin-dnd/android/src/main/java/com/sutauruki/studybites/dnd/DndRestoreReceiver.kt, src-tauri/plugins/tauri-plugin-dnd/permissions/autogenerated/commands/dnd_enable.toml, src-tauri/plugins/tauri-plugin-dnd/permissions/autogenerated/commands/dnd_request_access.toml, src-tauri/plugins/tauri-plugin-dnd/permissions/autogenerated/commands/dnd_restore.toml, src-tauri/plugins/tauri-plugin-dnd/permissions/autogenerated/commands/dnd_status.toml, src-tauri/plugins/tauri-plugin-dnd/permissions/autogenerated/reference.md, src-tauri/plugins/tauri-plugin-dnd/permissions/default.toml, src-tauri/plugins/tauri-plugin-dnd/permissions/schemas/schema.json, src-tauri/plugins/tauri-plugin-dnd/src/commands.rs, src-tauri/plugins/tauri-plugin-dnd/src/desktop.rs, src-tauri/plugins/tauri-plugin-dnd/src/error.rs, src-tauri/plugins/tauri-plugin-dnd/src/lib.rs, src-tauri/plugins/tauri-plugin-dnd/src/logic.rs, src-tauri/plugins/tauri-plugin-dnd/src/mobile.rs, src-tauri/plugins/tauri-plugin-dnd/src/models.rs: created the local Tauri plugin, Android implementation, desktop stub, and permissions.
2. Checks I ran (command and result):
 - cargo fmt --all --check; cargo test -p tauri-plugin-dnd; cargo test -p studybites-core; cargo check --workspace --all-targets; cargo clippy --workspace --all-targets -- -D warnings: PASSED.
 - cargo check -p tauri-plugin-dnd --target aarch64-linux-android: PASSED.
 - npm run typecheck; npm run build:web; git diff --check: PASSED.
 - ./gradlew :tauri-plugin-dnd:compileDebugKotlin --no-daemon: NOT RUN; no Gradle wrapper or system Gradle is installed.
 - Failures: none in the final checks.
3. Not verified:
 - Kotlin/Gradle compilation and Android device behavior.
4. Where I changed or went beyond the prompt, and why:
 - D40 records Android-only scope, priority default, conservative restore, and crash recovery.
5. Problems, risks and anything I guessed:
 - Full Tauri/Android build was not run per working rules; Android plugin Gradle integration appears in generated settings but Kotlin remains uncompiled here.
6. What the developer must do or test now (device checklist, secrets, settings):
 - On Android: grant DND access; enable the setting and start/end a study; kill the app mid-session and verify restore by planned end +2 minutes; manually change DND and verify StudyBites preserves that choice; test OEM settings behavior.
7. What is left:
 - Remaining parts of this task: Android DND device and Kotlin/Gradle verification.
 - Roadmap status: items 1, 2, 3, 11 [~]; items 4-10, 12-16 [ ].
 - Suggested next step: continue to Task F; return to the device checklist before marking item 3 complete.
READY FOR NEXT: yes
=== END REPORT ===

=== STUDYBITES HANDOFF REPORT ===
Task: F — Bug report pipeline | Status: partly done
Branch and last commit: ccr-cd109432-nnvymz 4d1c08c | CI: not run
1. What I did (one line per change):
 - Edited: .github/workflows/build-mobile.yml, .github/workflows/build-windows.yml, README.md, crates/studybites-core/src/lib.rs, crates/studybites-core/src/migrations.rs, crates/studybites-core/src/providers.rs, crates/studybites-core/tests/storage.rs, docs/DECISIONS.md, docs/PRODUCT_PLAN.md, docs/ROADMAP.md, src-tauri/src/lib.rs, src-tauri/src/phase2.rs, src-tauri/src/state.rs, src/App.tsx, src/api/index.ts, src/api/mock.ts, src/api/tauri.ts, src/api/types.ts, src/routes/SettingsPage.tsx, src/styles/app.css: wired opt-in reports, minimized diagnostics, encrypted outbox, and compile-time endpoint configuration.
 - New: .github/workflows/deploy-bug-worker.yml, crates/studybites-core/src/bugreport.rs, src/components/ReportBugDialog.tsx, src/lib/diag.ts, workers/bug-report/README.md, workers/bug-report/package-lock.json, workers/bug-report/package.json, workers/bug-report/src/index.ts, workers/bug-report/test/index.test.ts, workers/bug-report/tsconfig.json, workers/bug-report/wrangler.toml: added validation, redaction, email delivery, rate limits, tests and deployment instructions.
2. Checks I ran (command and result):
 - cargo fmt --all --check; cargo test -p studybites-core; cargo check --workspace --all-targets; cargo clippy --workspace --all-targets -- -D warnings: PASSED.
 - npm run build:web: PASSED.
 - cd workers/bug-report && npm test && npm run typecheck: PASSED (9 tests).
 - git diff --check: PASSED.
 - npm test from repository root: FAILED because no root test script exists; the Worker test command above passed from its directory.
 - Failures: `npm error Missing script: "test"` (wrong working directory; no code test failures).
3. Not verified:
 - Worker deployment/live Resend delivery; Settings UI, screenshot picker, clipboard, and offline queue on Windows or Android devices.
4. Where I changed or went beyond the prompt, and why:
 - D41 records opt-in reporting and diagnostic data minimization; generic error categories prevent raw error text from being uploaded.
5. Problems, risks and anything I guessed:
 - `onboarding@resend.dev` is test-only. Cloudflare KV rate counters are eventually consistent; concurrent requests can race.
6. What the developer must do or test now (device checklist, secrets, settings):
 - Configure the Worker secrets and `STUDYBITES_BUG_REPORT_URL` repository variable per workers/bug-report/README.md; deploy and verify delivery. Replace the test sender before public launch. On Windows and Android, test send, three resized screenshots, copy fallback, offline queue/retry, and privacy disclosure.
7. What is left:
 - Remaining parts of this task: Worker deployment and device verification.
 - Roadmap status: items 1, 2, 3, 4, 11 [~]; items 5-10, 12-16 [ ].
 - Suggested next step: deploy the Worker with owner-managed secrets, then continue Task G.
READY FOR NEXT: yes
=== END REPORT ===

=== STUDYBITES HANDOFF REPORT ===
Task: G — DeepSeek provider | Status: partly done
Branch and last commit: ccr-cd109432-nnvymz 62869a9 | CI: not run
1. What I did (one line per change):
 - README.md (edited): documented the four BYO-key providers, costs, and Gemini's current photo-transcription preference.
 - crates/studybites-core/src/ai.rs (edited): added DeepSeek identity, default priority, and key URL.
 - crates/studybites-core/src/providers.rs (edited): implemented DeepSeek text/vision calls, model listing, image support and credit-error mapping.
 - crates/studybites-core/tests/pipeline.rs (edited): covered provider identity, priority and default model.
 - crates/studybites-core/tests/study_flow.rs (edited): covered request shapes, aliases, image fallback, errors and model listing.
 - docs/DECISIONS.md (edited): added D42 with official API, errors, vision and pricing references.
 - docs/PRODUCT_PLAN.md (edited): documented providers, pricing and photo preference.
 - src-tauri/src/lib.rs (edited): registered the key-test command.
 - src-tauri/src/phase2.rs (edited): exposed DeepSeek in Settings and added a non-persisting key test.
 - src/api/mock.ts (edited): added DeepSeek and browser-preview key-test behavior.
 - src/api/tauri.ts (edited): connected the key-test IPC method.
 - src/api/types.ts (edited): added the typed key-test method.
 - src/routes/SettingsPage.tsx (edited): added key testing and DeepSeek help text.
2. Checks I ran (command and result):
 - cargo fmt --all --check; cargo test -p studybites-core; cargo check --workspace --all-targets; cargo clippy --workspace --all-targets -- -D warnings: PASSED.
 - npm run build:web; git diff --check: PASSED.
 - cargo test --workspace: NOT RUN to completion; stopped during a clean Tauri-library rebuild after several minutes, with no error output.
 - Failures: none in completed checks.
3. Not verified:
 - Live DeepSeek calls or key validation; no provider key was used.
 - Settings behavior in a running desktop app and photo transcription on a device.
4. Where I changed or went beyond the prompt, and why:
 - No unrelated scope changes. D42 records the verified API format, behavior and pricing.
5. Problems, risks and anything I guessed:
 - Pricing and available models can change; model selection is fetched live. The workspace test command did not complete.
6. What the developer must do or test now (device checklist, secrets, settings):
 - In Settings, test and save a DeepSeek key; select Flash and verify a photo transcription, then select Pro with another image-capable provider configured to confirm fallback. Check current DeepSeek pricing before use.
7. What is left:
 - Remaining parts of this task: live API and Settings/device verification.
 - Roadmap status: items 1-4 and 11 [~]; items 5-10 and 12-16 [ ].
 - Suggested next step: continue to Task H.
READY FOR NEXT: yes
=== END REPORT ===

=== STUDYBITES HANDOFF REPORT ===
Task: Codespace disk cleanup | Status: done
Branch and last commit: ccr-cd109432-nnvymz e888584 | CI: not run
1. What I did (one line per change):
 - target/ (ignored generated output, removed): Cargo build artifacts removed; cargo clean deleted the full 18.3 GB target tree.
 - src-tauri/gen/android/{app/build,build,buildSrc/build,.gradle} (ignored generated output, removed): removed Android Gradle outputs and caches; preserved generated project sources and signing properties.
 - dist/ (ignored generated output, removed): removed the 348 KB frontend bundle.
 - docs/STATUS.md (replaced): recorded cleanup details and remaining build implications.
2. Checks I ran (command and result):
 - Disk usage inspection (`du`, `df`): PASSED; about 18.3 GB recovered and 20 GB available.
 - Git status: PASSED; only the pre-existing `src/App.tsx` modification remains tracked.
 - Failures: none.
3. Not verified:
 - Rust Analyzer/build behavior after Cargo artifacts are regenerated.
4. Where I changed or went beyond the prompt, and why:
 - Removed ignored Android Gradle caches and the small `dist/` output as disposable generated files.
5. Problems, risks and anything I guessed:
 - Cargo cleanup removed the complete `target/` directory, including Rust Analyzer's cached build artifacts; later Rust checks will rebuild them.
6. What the developer must do or test now (device checklist, secrets, settings):
 - No immediate action. The next Rust analysis/build may take longer while artifacts are recreated.
7. What is left:
 - Remaining parts of this task: none
 - Roadmap status: item 1 [~], item 11 [~]; all other items [ ]
 - Suggested next step: continue the pending task; avoid rebuilding until needed.
READY FOR NEXT: yes
=== END REPORT ===

=== STUDYBITES HANDOFF REPORT ===
Task: H — Managed AI Worker | Status: partly done
Branch and last commit: 5a78c3c | CI: not run at the time; Rust checks to be confirmed by CI on the PR
1. What I did (one line per change):
 - workers/ai-proxy/src/index.ts (new): Worker that verifies the activation code signature, applies per-license UTC daily limits in KV, and proxies chat requests to the configured Gemini or DeepSeek key.
 - workers/ai-proxy/{package.json,package-lock.json,tsconfig.json,wrangler.toml,README.md} (new): Worker dependencies, config with a USAGE KV binding, and deployment instructions.
 - workers/ai-proxy/test/{index.test.ts,fixtures/license.json} (new): tests for signature checks, limits and routing, with no real provider key.
 - .github/workflows/deploy-ai-worker.yml (new): deploys the Worker on main when workers/ai-proxy changes, validates required secrets, then sets Worker secrets.
 - src-tauri/src/managed_ai.rs (new): Rust client that sends the stored activation code to the Worker and maps its responses.
 - crates/studybites-core/src/{ai.rs,providers.rs} and tests (edited): managed AI added as the final fallback only, after the BYO-key providers.
 - src-tauri/src/{lib.rs,keys.rs,phase2.rs,worker.rs} (edited): command registration, key handling, usage call and job-queue fallback wiring.
 - src/api/{mock.ts,tauri.ts,types.ts}, src/routes/SettingsPage.tsx (edited): managed AI status, plan usage display and browser mock.
 - README.md, docs/{DECISIONS.md,PRODUCT_PLAN.md,ROADMAP.md} (edited): documented the fallback, limits and D43.
 - .github/workflows/{build-windows.yml,build-mobile.yml} (edited): Worker URL configuration for release builds.
2. Checks I ran (command and result):
 - npm ci (repo root): PASSED
 - npm run typecheck: PASSED
 - npm run build:web: PASSED
 - workers/ai-proxy: npm ci, npm test (10 tests), npm run typecheck: PASSED
 - Rust checks (fmt, check, clippy, cargo test --workspace): NOT RUN locally (no Rust toolchain on this machine); to be confirmed by CI on the PR.
 - Failures: none in the checks that ran.
3. Not verified:
 - Real Cloudflare deployment, KV namespace creation and live usage limits.
 - Live Gemini/DeepSeek calls through the Worker; no provider key was used.
 - Settings usage display and fallback behavior on Windows or Android devices.
 - Concurrent KV counter races under real traffic.
4. Where I changed or went beyond the prompt, and why:
 - Managed AI is a last-resort fallback only; BYO-key providers keep priority. Recorded as D43 with the KV/limit caveats.
5. Problems, risks and anything I guessed:
 - KV increments are not atomic and usage is recorded before the upstream call, so the limits are best-effort, not billing-accurate.
 - A request is not independently bound to the activation code's device fingerprint.
6. What the developer must do or test now (device checklist, secrets, settings):
 - Create a KV namespace named USAGE in Cloudflare and paste its id into workers/ai-proxy/wrangler.toml.
 - Set repository secrets STUDYBITES_LICENSE_PUBKEY, STUDYBITES_AI_ADMIN_TOKEN, CLOUDFLARE_API_TOKEN, CLOUDFLARE_ACCOUNT_ID and at least one of STUDYBITES_AI_GEMINI_KEY / STUDYBITES_AI_DEEPSEEK_KEY, then run the deploy workflow.
 - On a device, use up the BYO-key providers and confirm managed AI answers, that Settings shows usage, and that the daily limit is enforced.
7. What is left:
 - Remaining parts of this task: KV id, secrets, deployment and live/device verification.
 - Roadmap status: items 1, 2, 3, 4, 11, 13 [~]; items 5-10, 12, 14-17 [ ].
 - Suggested next step: complete the Cloudflare setup above, then verify the fallback on a device.
READY FOR NEXT: yes
=== END REPORT ===

=== STUDYBITES HANDOFF REPORT ===
Task: Repo stabilisation after the Copilot work (Worker CI) | Status: done
Branch and last commit: chore/stabilise-workers-ci, this commit | CI: to be confirmed on the PR
1. What I did (one line per change):
 - workers/ai-proxy/vitest.config.ts (new): scopes vitest to test/**/*.test.ts so it no longer loads the repo-root vite.config.ts in a clean CI job.
 - workers/bug-report/vitest.config.ts (new): same file, for the bug-report Worker.
 - .github/workflows/ci.yml (edited): added a "workers" job that runs npm ci, npm test and npm run typecheck for workers/bug-report and workers/ai-proxy on the same push/PR triggers.
 - .github/workflows/deploy-ai-worker.yml (edited): added a pre-deploy check that fails with a clear message while workers/ai-proxy/wrangler.toml still contains SET_USAGE_KV_NAMESPACE_ID.
 - studybites-release.base64 (deleted): removed the empty tracked file from the repo root.
 - .gitignore (edited): ignores *.jks, *.keystore, *.p12 and *.base64.
 - docs/STATUS.md (replaced): reports reordered newest-last, plus the missing Task H report and this one.
 - docs/DECISIONS.md (edited): added D44 for the Worker CI/exclusion rules.
2. Checks I ran (command and result):
 - npm ci (repo root): PASSED
 - npm run typecheck: PASSED
 - npm run build:web: PASSED
 - workers/ai-proxy: npm ci, npm test (10 tests), npm run typecheck: PASSED
 - workers/bug-report: npm ci, npm test (9 tests), npm run typecheck: PASSED
 - Without the new vitest.config.ts, `npm test` in workers/ai-proxy picked up the root vite.config.ts and loaded the React plugin; with it, only the Worker config is used.
 - Rust checks (fmt, check, clippy, cargo test --workspace): NOT RUN locally (no Rust toolchain); to be confirmed by CI on the PR.
 - Failures: none in the checks that ran.
3. Not verified:
 - The green CI run itself, including the new "workers" matrix job and the deploy guard, which only run on GitHub.
 - Worker deployment against real Cloudflare.
4. Where I changed or went beyond the prompt, and why:
 - Also ran the bug-report Worker's install, tests and typecheck, because its vitest config was added in this task.
 - Added D44 to docs/DECISIONS.md, since the CI contract changed and the decision log is the place to say why.
5. Problems, risks and anything I guessed:
 - The deploy guard reads wrangler.toml with grep, so it only catches the literal placeholder; a wrong but non-placeholder KV id still reaches Cloudflare.
 - .gitignore cannot untrack already-committed files; nothing sensitive is tracked today.
6. What the developer must do or test now (device checklist, secrets, settings):
 - Watch the PR's CI run: the existing "check" job plus the new "workers" job must both be green.
 - Before deploying the managed AI Worker, create the USAGE KV namespace and replace the placeholder id in workers/ai-proxy/wrangler.toml.
7. What is left:
 - Remaining parts of this task: none; waiting on CI and the PR review.
 - Roadmap status: items 1, 2, 3, 4, 11, 13 [~]; items 5-10, 12, 14-17 [ ].
 - Suggested next step: merge the PR once CI is green, then complete the Cloudflare KV setup for Task H.
READY FOR NEXT: yes
=== END REPORT ===

=== STUDYBITES HANDOFF REPORT ===
Task: Outline updates (roadmap item 5) | Status: done, device checks pending
Branch and last commit: feature/outline-updates, this commit | CI: to be confirmed on the PR
1. What I did (one line per change):
 - docs/ROADMAP.md (edited): item 13 is now "Web version and managed AI"; accounts, project groups and study rooms follow as 14-16; group streaks and the libraries move to 17-18 and Nibble stays last at 19. Item 5 is [~].
 - docs/PRODUCT_PLAN.md (edited): new "Accounts, groups and study rooms" section.
 - crates/studybites-core/src/migrations.rs (edited): migration 008 adds scheme_topics.archived_at, scheme_topics.coverage and the outline_updates table.
 - crates/studybites-core/src/models.rs (edited): Topic carries archivedAt and coverage with serde defaults.
 - crates/studybites-core/src/topics.rs (edited): archived topics are hidden from list_flat/list_tree; adds list_all, list_archived, archive, restore and set_coverage.
 - crates/studybites-core/src/outline.rs (new): normalize, diff_outline, apply_outline_update, history, plus unit tests.
 - crates/studybites-core/src/study.rs, cbt.rs, preview.rs, subjects.rs (edited): archived topics' cards are excluded from sessions, resumes, due counts, CBT tests, previews and progress counts.
 - crates/studybites-core/src/lib.rs (edited): registers the outline module.
 - crates/studybites-core/tests/outline.rs (new): a rename keeps the id and links; an archived topic's cards are hidden then restored; re-importing does not duplicate.
 - crates/studybites-core/tests/storage.rs (edited): the migration test covers the new columns and table; table count is 18.
 - src-tauri/src/commands.rs, phase2.rs, lib.rs (edited): topic_archive, topic_restore, topics_archived, topic_set_coverage, outline_diff, outline_apply, outline_history.
 - src/api/types.ts, tauri.ts (edited): new types and commands.
 - src/api/mockOutline.ts (new): the same normalisation and matching in the browser mock.
 - src/api/mock.ts (edited): archive/restore/coverage, diff/apply/history, and archived rows hidden from sessions, CBT and progress.
 - src/routes/SchemeImportDialog.tsx (replaced): Changes stage with Added/Renamed/Moved/Removed/Restored, "Same as...", pinned "Apply update" footer and the summary line.
 - src/routes/SubjectPage.tsx (edited): per-topic menu (Taught/Self/Not set/Archive), badges, Archived topics section with Restore and guarded delete, "Outline last updated" with a short history.
 - src/styles/app.css (edited): styles for the menu, the archived list and the Changes screen.
 - README.md (edited): "When the outline changes".
 - docs/DECISIONS.md (edited): D45-D48.
2. Checks I ran (command and result):
 - npm run typecheck: PASSED
 - npm run build:web: PASSED
 - Rust fmt, check, clippy and tests: NOT RUN (no Rust toolchain here); CI runs them on the PR.
3. Not verified:
 - The seven device checks in section 6, on a real device.
 - Rust compilation, formatting and clippy until CI reports.
4. Where I changed or went beyond the prompt, and why:
 - Roadmap item 13 was "Managed AI Worker" and the web version was item 17. The prompt calls item 13 "web version and managed AI" and asks for Nibble last, so the two were merged into item 13; otherwise the web version would still come after Nibble.
 - Part B was not done; see section 5.
 - The import dialog's onSaved now takes a message instead of a count (internal prop only).
5. Problems, risks and anything I guessed:
 - PART B, workers/ai-proxy/wrangler.toml: it holds id = "PASTE_KV_ID_HERE", still the literal placeholder, so per the instruction I stopped and did not guess a KV namespace id. (On main the file still says SET_USAGE_KV_NAMESPACE_ID; the rename lives only on the older wip branch.)
 - Applying an empty decision set still writes an outline_updates row; the dialog disables Apply when nothing is selected.
 - A large rename (such as "Sets" to "Set theory") is deliberately not matched automatically; the "Same as..." pairing is the route.
6. What the developer must do or test now (device checklist, verified or not verified):
 1 import an outline into a new subject, same as before: NOT VERIFIED
 2 import a changed outline into a subject that has topics and see the Changes screen: NOT VERIFIED
 3 apply it: progress and cards kept, the removed topic archived: NOT VERIFIED
 4 the archived topic's cards no longer appear in a session: NOT VERIFIED
 5 restore it: NOT VERIFIED
 6 mark a topic Taught in class and Self-studied and see the badge: NOT VERIFIED
 7 no duplicate topics appear anywhere: NOT VERIFIED
7. What is left:
 - Remaining parts of this task: the seven device checks, and PART B needs the real KV namespace id.
 - Roadmap status: items 1, 2, 3, 4, 5, 11, 13 [~]; items 6-10, 12, 14-19 [ ].
 - Suggested next step: run the device checklist, then merge once CI is green.
READY FOR NEXT: yes
=== END REPORT ===
