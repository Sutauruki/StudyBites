=== STUDYBITES HANDOFF REPORT ===
Task: Working rules documentation fix | Status: done
Branch and last commit: ccr-cd109432-nnvymz 70a051b | CI: not run
1. What I did (one line per change):
 - docs/WORKING_RULES.md (new): added the ten working rules and empty handoff report template verbatim from the original prompt.
 - .github/copilot-instructions.md (edited): added docs/WORKING_RULES.md to required reading and replaced the old handoff-report reference.
 - docs/STATUS.md (replaced): saved this handoff report as required by the working rules.
2. Checks I ran (command and result):
 - git diff --check: PASSED
 - targeted marker and changed-file-scope validation: PASSED
 - CI: NOT RUN (documentation-only fix)
 - Failures: none.
3. Not verified (things I could not test, e.g. on a phone or in CI):
 - GitHub Actions CI execution.
4. Where I changed or went beyond the prompt, and why:
 - docs/STATUS.md was replaced because the working rules require saving the handoff report there.
5. Problems, risks and anything I guessed:
 - None.
6. What the developer must do or test now (device checklist, secrets, settings):
 - Review docs/WORKING_RULES.md as the shared source for future task handoffs.
7. What is left:
 - Remaining parts of this task: none
 - Roadmap status: all items 1-16 are [ ]
 - Suggested next step: proceed to Task C when ready.
READY FOR NEXT: yes
=== END REPORT ===
