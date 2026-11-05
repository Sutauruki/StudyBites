WORKING RULES (all tasks)
1. Read first: PROJECT_BRIEF-1.md, README.md, docs/DECISIONS.md, package.json scripts and .github/workflows/ci.yml (the exact checks CI runs).
2. Before coding each task, write a short plan: files to change, files to create, risks. Then proceed.
3. Work in small steps and commit after each with a clear message.
4. Codespaces hours are limited: do NOT run a full Tauri build. Run only what CI runs for the code you changed (cargo test -p studybites-core, clippy, typecheck, frontend tests if present). Never say a check passed unless you ran it and saw it pass. List anything you could not verify.
5. Do not guess. If unsure about an API format or library behaviour, choose the simplest option and log it in docs/DECISIONS.md, or leave a clearly marked TODO and say so.
6. Keep existing behaviour stable: do not change existing command signatures. New serde fields get #[serde(default)]. Migrations are append-only; never edit existing ones.
7. For every file you create or change, state its exact path and whether it is new or replaced.
8. Never require, hardcode or read an AI provider key from the environment. All AI calls use the existing in-app key settings (users paste their own key). Tests use the fake Http, never a real provider.
9. Anything platform-specific (Android) must compile and run unchanged on Windows and in the browser mock.
10. REPORT. At every STOP, and whenever you are blocked, your final message must be exactly the report below, filled in honestly. Also save the same text to docs/STATUS.md (replace the file each time) and commit it. Keep it under 500 words, plain text, no long diffs. Never claim a check passed or a feature works unless you ran it and saw it pass; say "not verified" instead.

=== STUDYBITES HANDOFF REPORT ===
Task: <letter and name> | Status: done / partly done / blocked
Branch and last commit: <branch> <short SHA> | CI: <passing / failing / not run / unknown>
1. What I did (one line per change):
 - <exact path> (new / replaced / edited): <what changed>
2. Checks I ran (command and result):
 - <command>: PASSED / FAILED / NOT RUN (why)
 - Failures: paste the exact error lines (the relevant 10 lines at most)
3. Not verified (things I could not test, e.g. on a phone or in CI):
 - <item>
4. Where I changed or went beyond the prompt, and why:
 - <deviation> (logged as D<number> in docs/DECISIONS.md)
5. Problems, risks and anything I guessed:
 - <item>
6. What the developer must do or test now (device checklist, secrets, settings):
 - <step>
7. What is left:
 - Remaining parts of this task: <list or "none">
 - Roadmap status: copy the current markers from docs/ROADMAP.md
 - Suggested next step: <one line>
READY FOR NEXT: yes / no (if no, say what blocks it)
=== END REPORT ===
