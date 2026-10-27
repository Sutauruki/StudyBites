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
