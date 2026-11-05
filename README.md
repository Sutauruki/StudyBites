# StudyBites

A distraction-free desktop study app. It turns your own class materials into
short study sessions ("bites") and remembers your progress day to day with
spaced repetition.

Proprietary software — see [`LICENSE`](LICENSE). Not open source.

**Current state: Phase 2 complete** — subjects and the scheme of work, plus
uploads, AI-written notes and cards, spaced-repetition study sessions, Explain,
and update checking. Activation and device-locked encryption arrive in Phase 3.

---

## How to get the Windows installer

You do not need a Codespace, Rust, or Visual Studio Build Tools for this. Every
push to `main` builds the installer on a GitHub Windows runner.

1. Wait for the green check on the latest commit to `main`:
   **GitHub → this repo → Actions → "Build Windows installer"**.
   The first build takes roughly 10–20 minutes; later ones are faster thanks to
   caching.
2. Go to **GitHub → this repo → Releases → "Latest build"** and download the
   `.exe`. It is replaced on every build, so that link is always the newest one.
   (The same file is also attached to the Actions run as an artifact, kept for
   7 days.)
3. On the EliteBook, run the `.exe`. The app is not code-signed, so Windows
   SmartScreen will warn you: choose **More info → Run anyway**.

To cut a versioned release instead of a rolling one, push a tag:

```bash
git tag v0.1.0
git push origin v0.1.0
```

That publishes a normal GitHub Release with the installer attached.

If no build has run yet, start one by hand from
**Actions → "Build Windows installer" → Run workflow**.

### Upgrades and existing data

The Windows installer keeps the stable application identity
`com.sutauruki.studybites`, installs per user, and stores the database,
original files, settings, and licensing data in the platform app-data
directory rather than beside the executable. Running a newer installer over
the existing installation updates the program files without deleting that
data. Versioned releases must use increasing `vMAJOR.MINOR.PATCH` tags.

Android builds target aarch64 only, so the APK is 64-bit and will not install
on some older or budget phones that run 32-bit Android. The Android workflow
publishes every main build to the **Latest build** pre-release. Download the
`StudyBites_<version>_arm64.apk` from **GitHub -> Releases -> Latest build**,
then install it over the old version rather than uninstalling first, because
uninstalling Android removes app data. Tagged releases use the same filename
pattern.

Android updates require the same application ID and the same production
keystore. Configure these GitHub Actions secrets before building:

1. Create a PKCS12 keystore with one password (the command prompts for it):

   ```bash
   keytool -genkeypair -v -storetype PKCS12 \
     -keystore studybites-release.jks -alias studybites -keyalg RSA \
     -keysize 2048 -validity 10000
   ```

2. Set `ANDROID_KEY_BASE64` to the one-line output of
   `base64 -w0 studybites-release.jks`, `ANDROID_KEY_ALIAS` to `studybites`,
   and `ANDROID_KEY_PASSWORD` to that same keystore password. The signing
   script uses one password for both the keystore and key.
3. Confirm `STUDYBITES_LICENSE_PUBKEY` is also configured if activation is
   enabled.

Never commit the `.jks` file. Keep a secure backup of it and its password;
losing the production keystore prevents updates over existing installations.

---

## Working on the code (Codespaces or any Linux box)

The native window needs Windows, but nothing else does. The UI runs in a browser
against a mock backend, and all the Rust logic is tested without a WebView.

```bash
npm install          # once
npm run dev:web      # UI on port 5173, mock backend, no native shell needed
```

The mock keeps synthetic data in `localStorage`, and the title bar shows a
"browser preview — mock data" badge so there is no confusing it with the real
app.

Checks, the same ones CI runs on every branch and pull request:

```bash
npm run typecheck                      # TypeScript
npm run build:web                      # typecheck + production bundle
cargo test -p studybites-core          # storage and domain logic (fast, no system deps)
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

`cargo check` and `cargo test` across the *whole* workspace also compile the
Tauri crate, which on Linux needs the WebView toolchain:

```bash
sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev libsoup-3.0-dev libssl-dev patchelf
cargo test --workspace
```

The devcontainer in `.devcontainer/` installs those for you.

### Building the real desktop app

Only on Windows, and normally only in CI:

```bash
npm run build        # tauri build -> NSIS installer in target/release/bundle/nsis
```

---

## Layout

```
crates/studybites-core/   Rust: encrypted DB, migrations, subjects, topics. No Tauri.
src-tauri/                Tauri v2 shell: IPC commands, config, capabilities, icons.
src/                      React + TypeScript UI.
  api/                    The IPC bridge, plus the browser mock backend.
  routes/                 Subjects list, subject detail (outline editor), settings.
  styles/                 Design tokens and the stylesheet.
docs/DECISIONS.md         Why things are the way they are.
.github/workflows/        ci.yml (Linux, cheap) and build-windows.yml (installer).
```

The split matters: everything that touches data lives in `studybites-core` with
no Tauri dependency, so it builds and tests anywhere. `src-tauri` is a thin
wrapper that locks the database and forwards calls.

---

## Using it

### First run

1. **Create a subject** for one of your courses.
2. **Add an AI key** in Settings. StudyBites uses your own free key — Gemini is
   tried first, with Groq and OpenRouter as fallbacks when a daily limit is
   reached. Settings links straight to where each one is issued. Keys go into
   Windows' credential store, never into the app's database.
3. Optionally **sketch the scheme of work** for the subject: topics, subtopics
   and week numbers. You can skip this and add topics as you go.

### Every day

1. **Add material** — a lecture PDF, a DOCX, or a text file.
2. **Confirm the topic.** If there is a scheme of work, the AI pre-selects the
   topic it thinks the material belongs to; confirm it, pick another, or choose
   "Something else" and describe it. Nothing is filed silently.
3. StudyBites reads the file, splits it by heading and paragraph, and has the
   AI write compact notes and a few cards per section. This runs in the
   background — the window stays usable, and you can study while it works.
4. **Start a bite** from the home screen: 5, 10 or 15 minutes. Due reviews come
   first, then a few new cards, capped so it never feels heavy.
5. When the time is up the session ends with a short summary. Tomorrow, what
   you studied comes back when it is due.

During a session: **Space** reveals, **1–4** grade (Again / Hard / Good / Easy),
**Esc** ends.

### Other things it does

- **Ask about a subject** — answers come only from your own uploaded notes for
  that subject, and the notes used are listed so you can check them.
- **Progress bars per topic**, from how well the cards have stuck.
- **Download original** gives back any file you uploaded, byte for byte.
- **Check for updates** in Settings tells you when a newer build exists. It
  never downloads or installs anything by itself.

### If the daily AI limit is reached

Processing pauses and resumes by itself after the limit resets — nothing is
lost, and nothing has to be re-uploaded. Studying keeps working completely
offline in the meantime.

### On the database key

Phase 1 uses a fixed development key that lives in the source. It is **not** a
secret and gives no real protection — it exists so the on-disk format is already
encrypted, making Phase 3 a key swap rather than a data migration. Phase 3
derives the key from the device fingerprint and the activation payload, after
which copying the database to another machine stops working.

---

## Licensing and activation

StudyBites is sold per device. A copy works only on the device it was
activated for, so passing the installer or the app folder to a friend gets
them nothing.

### One-time setup (owner)

Activation needs an Ed25519 key pair. **The private half must never be
committed to this repository or shipped inside the app.** Generate it once,
on your own machine, from the admin tool:

```bash
license-tool keygen --key owner.key
```

That prints a public key. Add it to this repository as an Actions secret
named `STUDYBITES_LICENSE_PUBKEY` (Settings → Secrets and variables →
Actions). The next Windows build compiles it in and requires activation.

Back `owner.key` up somewhere safe. It cannot be recovered, and generating a
new one invalidates every licence already sold. The tool refuses to overwrite
an existing key file for that reason.

Until the secret is set, builds still work — they run unlicensed and say so
in Settings → About. Nothing breaks while you are getting set up.

`license-tool.exe` is published as a workflow artifact by the Windows build,
so you do not need Rust installed to issue codes.

### Activating a copy (with a buyer)

1. The student installs StudyBites, accepts the agreement, and reads you the
   **request code** from the activation screen — about 23 characters.
2. You run:

   ```bash
   license-tool issue --key owner.key --request <REQUEST CODE> --buyer "Their Name"
   ```

3. Send them the **activation code** it prints. It is long, so paste it into
   a message rather than reading it out; the app's box accepts a paste.
4. They paste it in and the app unlocks. No internet is involved at any point.

Issued licences are recorded in `owner.licenses.json` beside the key, which
is gitignored. `license-tool list --key owner.key` shows them.

### Reinstalls

Reinstalling on the same device works with the same activation code —
re-issuing produces the identical code, by design, because the code is half
the key to that student's database. Issuing a *new* secret for a machine
that already has one would destroy their notes, so the tool deliberately
reuses the old one.

A new device, or a Windows reinstall, needs a new code.

### What this does and does not do

Be clear-eyed about it: **no software running on a machine the user controls
is unbreakable.** Someone determined and skilled could patch the binary. The
goal is narrower and achievable — make *copying* useless:

* The database and the stored originals are encrypted with a key derived from
  the machine and the licence. Copying the data folder to another device produces
  a different key, and the files simply do not open.
* An activation code for one machine is refused on another, even though its
  signature is genuine.
* A forged or edited code fails signature verification.

So casual sharing — the actual risk among students — gets nowhere, and
cracking it is more work than the app costs.

## Privacy

No telemetry, no analytics, no crash reporting. Nothing leaves the device in
Phase 1. From Phase 2, material is sent only to the AI provider you configure
with your own API key.
