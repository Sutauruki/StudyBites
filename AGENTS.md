# StudyBites: instructions for any coding agent

StudyBites is an offline-first AI study app: Tauri v2 + Rust core (crates/studybites-core) + React/TypeScript (src/), with Windows and Android builds and two Cloudflare Workers (workers/bug-report, workers/ai-proxy).

READ FIRST every session: PROJECT_BRIEF-1.md, docs/WORKING_RULES.md, docs/DECISIONS.md, docs/ROADMAP.md, docs/PRODUCT_PLAN.md, docs/STATUS.md, docs/UI_GUIDE.md.

LOCAL MACHINE (low-end Windows laptop, slow internet, no Rust installed):
- Do NOT run cargo, tauri, gradle or full builds. GitHub CI runs the Rust and Kotlin checks (fmt, check, clippy, cargo test --workspace) on every branch push and pull request.
- Local checks you CAN run: npm run typecheck, npm run build:web, and in each workers/<name> folder: npm ci, npm test, npm run typecheck.
- View the UI with npm run dev:web (mock backend in src/api/mock.ts, which must mirror the Rust rules).
- Install dependencies once. Do not add packages without logging why in docs/DECISIONS.md.

WORKFLOW: work on a branch, never push to main; small commits; open a PR; read the CI result before calling a task done. Never claim a check passed unless you ran it or CI shows it passed.

NEVER commit secrets: no API keys, no keystore (.jks/.p12), no license private key. No AI key goes inside the app: users paste their own, or it lives as a Worker secret.

At the end of every task, APPEND the handoff report from docs/WORKING_RULES.md to docs/STATUS.md (newest last; this overrides the "replace" wording there).