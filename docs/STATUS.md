=== STUDYBITES HANDOFF REPORT ===
Task: U — Android release asset and whole-app mobile UI pass | Status: partly done
Branch and last commit: ccr-cd109432-nnvymz c166858 | CI: not run
1. What I did (one line per change):
 - .github/workflows/build-mobile.yml (edited, U1): names exactly one signed APK, verifies its certificate, removes stale latest APKs, publishes the named file, and writes a release summary.
 - README.md (edited): documents StudyBites_<version>_arm64.apk and the Screen guide.
 - docs/DECISIONS.md (edited): added D36 for Android assets and D37 for Screen.
 - src/components/Screen.tsx (new): added the full-height scrolling body and safe-area footer primitive.
 - src/components/ui.tsx (edited): made Modal use Screen with pinned actions and mobile bottom-sheet layout.
 - src/routes/OnboardingPage.tsx, src/App.tsx (edited): moved gates and startup errors to Screen; gate actions are pinned.
 - src/routes/StudyPage.tsx, src/routes/CbtPage.tsx (edited): pinned study and CBT actions above phone navigation.
 - src/styles/app.css, index.html (edited): added mobile viewport behavior, safe areas, contained scrolling, touch sizing, no sideways page scroll, and bottom-nav offsets.
 - .github/copilot-instructions.md, docs/UI_GUIDE.md (edited/new): documented the primitive and mobile rules.
 - docs/ROADMAP.md (edited): marked item 11 [~] because device/build verification is outstanding.
2. Checks I ran (command and result):
 - npm run typecheck: PASSED
 - npm run build:web: PASSED
 - git diff --check: PASSED
 - Frontend tests: NOT RUN (no frontend test script or test files exist)
 - Failures: none
3. Not verified:
 - Android workflow/build, apksigner output, and GitHub release behavior.
 - Visual results on device, including keyboard, landscape, large font and safe-area behavior.
4. Where I changed or went beyond the prompt, and why:
 - Modal was made a Screen-backed bottom sheet so all existing dialogs share one footer implementation; logged as D37.
5. Problems, risks and anything I guessed:
 - The visual pass is code-reviewed only; exact device rendering is not verified.
6. What the developer must do or test now (device checklist, secrets, settings):
 - Verify agreement fills the screen with Accept at bottom; Activate stays above keyboard; study reveal/grade and CBT navigation stay at bottom; dialog confirm stays at bottom; Settings is not hidden under nav; landscape and Large system font work.
 - Run the Android workflow with signing secrets and confirm the named APK installs over the prior signed build.
7. What is left:
 - Remaining parts of this task: device visual verification and Android release build verification
 - Roadmap status: item 11 [~]; all other items 1-10 and 12-16 are [ ]
 - Suggested next step: test the Android workflow and the checklist on 320/360/390/412px phones.
READY FOR NEXT: yes
=== END REPORT ===
