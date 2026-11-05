# Technical decisions

A running log of choices worth remembering, newest phase last. Each entry says
what was decided and, where it matters, what it rules out later.

## Phase 1 — Shell & foundations

### D1. The workspace is split into `studybites-core` and `src-tauri`

All storage and domain logic lives in `crates/studybites-core`, which has no
Tauri dependency. `src-tauri` holds only thin IPC wrappers.

Why: on Linux (Codespaces, the CI branch job) anything that links Tauri needs a
WebView toolchain. Keeping the logic separate means `cargo test -p
studybites-core` runs anywhere, in about a second, with no system libraries. It
also keeps the rule from Brief §10 honest — every IPC input is validated in one
place.

### D2. SQLCipher keys must be set with a literal `PRAGMA key` statement

`rusqlite`'s `pragma_update(None, "key", …)` appears to work — a new database is
created and written — but the file cannot be reopened afterwards: every later
read fails with `NotADatabase`. Using `execute_batch("PRAGMA key = '…'")`
instead works correctly.

This cost real debugging time, so it is written down. `db.rs` escapes `'` in the
key for the literal; Phase 3 will pass the 32-byte derived key as `x'<hex>'`,
which needs no escaping at all.

Verified by a test that asserts the file does not start with `SQLite format 3`
and that the wrong key returns `CoreError::Locked`. That test is the Phase 3
device-locking guarantee in miniature.

### D3. Integer primary keys, not UUIDs

Everything is single-user and local to one device, and there is no sync, so
`INTEGER PRIMARY KEY` is enough. It saves a dependency and keeps rows small.

If sync or multi-device ever happens, this is the decision to revisit.

### D4. Timestamps come from SQLite, not a Rust clock

Columns default to `strftime('%Y-%m-%dT%H:%M:%fZ','now')`. That is one fewer
crate (no `chrono`/`time`), and the format is sortable ISO-8601 UTC.

FSRS in Phase 2 will need real date arithmetic, which is when a date crate can
be justified — not before.

### D5. The whole Phase 1–4 schema ships in migration 001

All eleven tables from Brief §5 are created up front, even though Phase 1 only
reads and writes three of them. Later phases add rows, not tables, so the
owner's database survives upgrades without a migration on every release.

Migrations are append-only and tracked with SQLite's `user_version`. A database
from a *newer* app version is refused rather than silently downgraded.

### D6. Topics nest exactly one level deep

Scheme of work means topics and subtopics; arbitrary depth would complicate
reordering, mastery roll-up and the "next topic" logic in Phase 4 for no real
gain. Attempting to nest deeper is a validation error, not a silent flatten.

### D7. Reorder is all-or-nothing

`topics::reorder` requires the caller to pass exactly the ids currently in that
sibling group. A list that is short, padded or from another group is rejected
and nothing changes, instead of applying a partial reorder. The UI's up/down
buttons go through `nudge`, which builds the full list itself.

### D8. No state library, no router, no UI kit

Phase 1 has three screens, so a `useState` union type stands in for a router,
and plain CSS with tokens covers the styling. Current frontend bundle: about
160 KB of JS and 7 KB of CSS, uncompressed.

Zustand gets added when there is state that genuinely spans screens — the study
session in Phase 2 is the likely trigger, not before.

### D9. The mock backend mirrors Rust's *rules*, not just its shapes

`src/api/mock.ts` re-implements trimming, duplicate-name rejection, the nesting
limit and the strict reorder check. The point is that UI work done in a browser
meets the same error messages it will meet in the real app, so empty and error
states are built against reality. It persists to `localStorage` and holds only
synthetic data.

`isDesktop` picks the real backend by looking for `__TAURI_INTERNALS__`.

### D10. `Option<Option<T>>` does not cross the IPC boundary

"Leave the week alone" and "clear the week" cannot both be expressed by a JSON
`null`, so `topic_update` takes an explicit `clear_week` flag. The core function
keeps its `Option<Option<i64>>` signature, which is honest in Rust.

### D11. A fixed development database key, replaced in Phase 3

`DEV_KEY` in `db.rs` is a constant and is not a secret — it is in the source and
provides no protection. It exists so that the on-disk format is already
encrypted, making Phase 3 a key swap rather than a data migration.

Phase 3 replaces it with `HKDF-SHA256(device_id || license_secret)` per Brief §9.

### D12. Icons are generated, not designed

`src-tauri/icons/*` are generated placeholders (a rounded square with a bite
taken out of it) so the Windows bundle has the `.ico` it requires. Worth
replacing with a real mark before anything ships to another student.

### D13. CI splits cheap Linux checks from the Windows build

Branches and pull requests run `ci.yml` on Ubuntu: frontend typecheck and
build, `cargo fmt --check`, `cargo check`, clippy with `-D warnings`, and
`cargo test`. Only `main`, version tags and manual runs spend Windows minutes in
`build-windows.yml`, which cancels superseded runs and ignores docs-only
changes (Brief §11.1).

The Windows job warns, rather than fails, when the installer exceeds the 40 MB
budget — Phase 1 has no embedding model yet, so a hard failure would be noise.

### D14. SQLCipher vendors its own OpenSSL

`rusqlite`'s `bundled-sqlcipher` feature compiles SQLCipher from source but
links the *system* OpenSSL. That works on Linux and fails on a GitHub
`windows-latest` runner, which has no OpenSSL for the build script to find:

```
Missing environment variable OPENSSL_DIR or OPENSSL_DIR is not set
```

`libsqlite3-sys` exposes only two crypto routes — system OpenSSL, or
`bundled-sqlcipher-vendored-openssl`, which builds OpenSSL from source as part
of the crate. (SQLCipher itself can use LibTomCrypt, and `sqlcipher-src` even
bundles it, but no feature exposes that choice.)

We use the vendored route on every platform. Pointing `OPENSSL_DIR` at whatever
the runner image happens to ship would work until the image changed; vendoring
makes the build depend on nothing outside the lockfile, which matters for a
project whose builds the owner cannot debug locally.

The costs, accepted: a slower first build (OpenSSL compiles from source, then
`rust-cache` keeps it warm) and a few MB of static OpenSSL in the binary.
Building OpenSSL on Windows needs NASM and Perl — the runner image has both,
and the workflow installs them if that ever stops being true.

### D15. Release upload paths use forward slashes, and must match a file

The first fully successful Windows build published a release with no installer
attached: the tag existed, the job was green, and there was nothing to
download.

`softprops/action-gh-release` treats `files` as a glob pattern, and in glob a
backslash escapes the next character. The path PowerShell produces,

```
D:\a\StudyBites\StudyBites\target\release\bundle\nsis\StudyBites_0.1.0_x64-setup.exe
```

therefore matches nothing. Confirmed against the glob library directly: the
backslash form returns 0 matches, the forward-slash form returns 1.
`actions/upload-artifact` uses a different glob implementation that handles
Windows paths, which is why the artifact uploaded correctly from the same path
and hid the problem.

So the collect step now emits forward slashes, and both release steps set
`fail_on_unmatched_files: true`. The default is `false`, which is what turned a
broken upload into a passing build — the kind of failure that costs a whole
round trip to notice.


## Phase 2 — Ingestion, AI and studying

### D16. FSRS is implemented directly, not taken from the `fsrs` crate

The crate pulls 66 dependencies including `ndarray` and `rayon`, because it
also *trains* FSRS parameters from a review history. StudyBites only
schedules, with the published defaults, so the training half would be weight
and heat on a dual-core laptop for nothing. The brief allows "a port of
ts-fsrs logic"; `fsrs.rs` is that port, about 150 lines and no dependencies.

Revisit if per-user parameter optimisation is ever wanted — that is the one
thing the crate buys.

### D17. Embeddings are lexical for now, behind an `Embedder` trait

The brief asks for a bundled `all-MiniLM-L6-v2` ONNX model. That is ~23 MB
plus an ONNX runtime and a tokenizer, against a 2.8 MB installer, and it is
the one component that could not be verified on Windows from the development
container — immediately after three rounds of Windows build failures.

What is implemented is the storage and search architecture the brief
specifies exactly: vectors as BLOBs, brute-force cosine similarity in Rust,
retrieval scoped to one subject. The vectors come from `LexicalEmbedder`, a
deterministic hashed bag-of-words with sub-linear weighting and a light
stemmer. Good enough to pull the right handful of notes from one subject's
few hundred, and it needs no model and no new dependency.

`Embedder` is the seam. Swapping in MiniLM later is one new impl plus a
re-embed; no schema change, no migration. **This is the one part of Phase 2
that ships lighter than the brief asks, and it is a deliberate trade.**

### D18. HTTP is `ureq` with rustls, not `reqwest`

`ureq` is blocking, which suits a background worker thread, and far smaller.
rustls means the HTTP stack needs no system TLS — and crucially does not drag
in a second OpenSSL alongside the one SQLCipher vendors (D14).

### D19. The provider layer is tested without a network

`providers::Http` is a trait; the real impl wraps `ureq`, and the tests use
canned responses. That is what makes it possible to test quota fallback,
the single retry on a garbled reply, and the rule that a known-exhausted
provider is never called — none of which could be tested against live free
tiers without burning the daily allowance.

### D20. One worker thread, woken on demand

Free tiers are rate-limited per minute, so parallel workers would only trip
the limits sooner. A single thread drains the queue and emits progress
events; the window never blocks. Jobs left `running` by a crash are requeued
at startup.

### D21. A quota pause is not a failure

When every provider is out, the job is parked until the reset time and the
attempt is given back, so waiting on a free tier never burns a retry or
marks an upload failed. Studying keeps working entirely offline meanwhile —
nothing in the study loop touches the network.

### D22. Update checking notifies, never installs

The owner chose notify-only. Worth recording why the obvious alternative
does not work: the repository is private, so even *reading* its releases
needs a token, and Tauri's updater would need that token inside the binary,
where anyone who buys a copy could extract it. That contradicts Brief 7.2
and undermines Section 8.

So the check uses the user's own GitHub token from the OS keychain, exactly
like the AI keys. No token means update checks are simply off, explained in
Settings. Nothing is ever downloaded or installed automatically.

If auto-update is wanted later, the clean route is a second, public
repository holding only installers and a manifest — source stays private,
no token ships, and Tauri's signed updater can be used properly.

### D23. Keys live in the OS keychain and never reach the frontend

`keyring` on Windows is DPAPI-backed. The WebView can ask *whether* a key is
set and can set one; it can never read one back. Every provider call happens
in Rust.

### D24. The device id comes from `MachineGuid` alone

Brief 8.1 asks for several hardware identifiers hashed together, with a
threshold so that swapping one part does not lock anyone out. Taken
literally that cannot work, because Brief 9 also makes `device_id` half the
key material for the database: a *threshold* match says "close enough", but
a key is either reproduced exactly or the data is gone. Any component that
can drift cannot be in the key.

So the two jobs are separated:

* `device_id` = SHA-256(salt, `MachineGuid`). It survives a new disk, a new
  CPU, more RAM and a new motherboard, and changes only when Windows itself
  is reinstalled — at which point the database is gone anyway. This is what
  the licence binds to and what the keys derive from.
* The CPU, baseboard, BIOS and system-volume identifiers are still collected
  and hashed, recorded at activation, and compared on each launch. They are
  never key material. They drive one honest sentence in Settings when the
  machine has changed, and nothing else.

This meets the brief's actual goal — do not punish a hardware swap — better
than thresholding would, because the tolerance is total rather than partial.

### D25. The licence is stored outside the encrypted database

It has to be: the database key is derived from the licence, so the licence
must be readable before the database can be opened. It sits beside it as
`license.txt`, in plain text, with the component profile in
`activation.json`.

Plain text is not a weakness here. The code is signed, so altering it only
makes it fail verification, and it is bound to one machine, so copying it
achieves nothing. Keeping it readable means a student who reinstalls can
recover their own code from the folder instead of asking for it again.

EULA acceptance does go in the database, as Brief 13 asks. The agreement is
accepted before activation, so the timestamp waits in memory and is written
the moment a database exists.

### D26. Activation codes are compact binary, not JSON

An Ed25519 signature is 64 bytes, which is already 103 characters of base32
before any content. A JSON payload would have pushed a code past 300
characters. The signed body is therefore a fixed binary layout — 42 bytes
plus the buyer's name — giving a code of about 190 characters.

The alphabet is Crockford base32: no I, L, O or U, decoding is
case-insensitive, and I/L read as 1 while O reads as 0. A student reading a
code off a phone screen cannot make the classic 1/I or 0/O mistakes. The
request code going the other way is only 23 characters, short enough to read
down a phone line.

### D27. A build without the public key runs unlicensed, and says so

The public key is compiled in from the `STUDYBITES_LICENSE_PUBKEY`
repository secret, so no part of the key pair is ever in this repository.
A build made without it — a local `cargo run`, or CI before the owner has
generated a key — cannot verify anything.

Such a build runs on the development key and states plainly in Settings that
it is a development build. The alternative, refusing to start, would mean
development and CI are blocked on a secret existing; pretending to be
licensed would be worse still. Activation becomes mandatory the moment the
secret is set, with no code change.

### D28. Activating re-keys an existing database rather than replacing it

Builds before licensing used the development key. Turning activation on
would otherwise strand the subjects, cards and review history of anyone
already using the app. `Db::open_or_rekey` notices a database that only
opens with the old key and runs `PRAGMA rekey` to the derived key in place.

### D29. Re-issuing a code must reproduce the same secret

Brief 8.3 decides that reinstalling on the same device works with the same
code. That requires more than accepting the old code: the licence secret is
half the key material, so issuing a *fresh* secret for the same machine
would derive a different database key and silently destroy the student's
data.

`license-tool issue` therefore looks the device up in its ledger and, if it
has issued for that machine before, recovers the original secret out of the
stored code rather than generating a new one. Issuing twice for the same
machine and buyer produces a byte-identical code.

### D30. One SQLCipher connection is held open for the life of the process

SQLCipher reference-counts its crypto provider: the first keyed connection
registers it, and closing the last one frees it. A process that opens and
closes short-lived connections across threads can have one thread free the
provider while another is attaching, which fails with `sqlcipher not
initialized` and surfaces as a spurious `Locked`. This was seen once in CI,
in the test suite, which does exactly that.

A single keyed in-memory connection is now created once and never dropped,
keeping the count above zero. It is never used for anything.

Stated honestly: the failure does not reproduce on the development machine —
50 runs of the affected binary passed both with and without the change — so
this is reasoned from SQLCipher's lifecycle rather than confirmed against a
reproduction. The cost is one idle connection, so it cannot make things
worse, but it should not be called proven until CI stays green over time.

Separately, `Db::prepare` no longer flattens every failure into `Locked`:
only a failed read *after* keying means a wrong key. A cipher that refuses
to start now reports its real error, which is what made this diagnosable.

### D31. Typed answers are marked locally first, and the AI only decides

Writing an answer from memory is a far stronger test than revealing one and
rating yourself. The cost is that marking wants an AI request per card, while
the study loop has been offline and free (D21).

So marking runs in two stages. A blank answer, an answer that matches word
for word, and pure noise are decided locally and instantly. Everything else —
whether different words mean the same thing, and how an examiner would want
it written — goes to the AI, which is where the value is anyway.

The thresholds are deliberately lopsided. Marking something correct when it
is not teaches the student the wrong thing, so the local "correct" rule is
strict and anything doubtful is passed on rather than guessed at. Expect most
real attempts to reach the AI; the local pass exists to stop the trivial cases
costing a request each, not to avoid the work.

### D32. Marking never suggests Easy

FSRS wants to know how hard a card was to *recall*. Marking knows how *right*
the answer was. They are related but not the same: someone who writes well can
produce a correct answer they barely remembered.

So the suggested rating tops out at Good, and the student can always override
it. Only they can say an answer came effortlessly. Mapping a high score to
Easy would inflate every interval for anyone who writes fluently, which is
exactly the wrong failure.

### D33. A practice test is not a review

CBT tests touch neither `card_state` nor `review_log`. A forty-question mock
answered against a clock would otherwise reschedule half the deck on the
strength of guesses, and a guessed-right answer would push a card the student
does not actually know weeks into the future.

Practice is recorded in its own tables. It is for the student to see where
they stand, not for the scheduler to act on.

Two smaller choices fall out of the same thinking:

* **Questions come from cards the student already has**, so a test costs no
  AI requests and works offline. Preview cards are excluded — they are
  labelled "not from your class" precisely because they might not be.
* **The answer key does not reach the WebView until the paper is submitted.**
  `cbt_start` returns questions only; `cbt_submit` returns the marking. An
  exam whose answers are already sitting in the page is not an exam.

## Release and roadmap decisions

### D34. Android main builds are signed and published as the rolling latest release

Every Android build uses the production PKCS12 keystore, including builds from
`main`, so the APK can be installed over the previous version. The workflow
fails with the missing secret name when signing is not configured, rejects an
`-unsigned.apk`, and publishes the signed APK to the rolling `latest`
pre-release. The build remains aarch64-only for now, so 32-bit Android phones
are explicitly unsupported. The keystore is never committed and must be
backed up outside the repository.
