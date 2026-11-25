# Managed AI Worker

This Cloudflare Worker verifies StudyBites activation codes, applies daily
per-license limits, and forwards requests to server-configured Gemini or
DeepSeek keys. The app sends its saved activation code as a bearer token; no
provider key is shipped to the app or sent by the app. The Worker does not
attest the device fingerprint, so license verification here is not an
independent device lock.

## One-time setup

1. Create a Cloudflare account with Workers and Workers KV enabled. In the
   Cloudflare dashboard, create a KV namespace named `USAGE`, then copy its ID
   into `wrangler.toml` as the binding `id` and commit that non-secret ID. The
   id is 32 lowercase hex characters; the deploy workflow refuses anything else
   (an unset placeholder, a truncated id, a name typed in by mistake).
2. Set these GitHub Actions secrets:
   - `CLOUDFLARE_API_TOKEN` with Workers and KV edit permissions
   - `CLOUDFLARE_ACCOUNT_ID`
   - `STUDYBITES_LICENSE_PUBKEY` (the same public key used to build the app)
   - `STUDYBITES_AI_ADMIN_TOKEN` (a unique, high-entropy token for `/admin/plan`)
3. Optionally add one or both server-side provider keys:
   - `STUDYBITES_AI_GEMINI_KEY`
   - `STUDYBITES_AI_DEEPSEEK_KEY`

   At least one provider key is needed for AI requests. To remove an existing
   key, delete it with `npx wrangler secret delete GEMINI_API_KEY` or
   `npx wrangler secret delete DEEPSEEK_API_KEY` in this directory.
4. Run **Deploy managed AI Worker** from Actions or push a change to this
   directory on `main`. The workflow tests and typechecks before deployment.
5. Set the repository variable `STUDYBITES_AI_URL` to the deployed HTTPS Worker
   root URL, such as `https://studybites-ai.<account>.workers.dev`. Windows and
   Android release builds compile this endpoint in. Builds without the
   variable keep managed AI disabled.

## Plans and administration

`/v1/status` and `/v1/ai` require the student's activation code in an
`Authorization: Bearer ...` header. The status route reports UTC daily usage
and limits; `/admin/plan` requires the separate `STUDYBITES_AI_ADMIN_TOKEN`
bearer token and accepts a decimal `licenseId` with plan `free`, `2000`,
`5000`, or `10000`.

The Worker uses `USAGE` KV keys `plan:<licenseId>` and
`usage:<licenseId>:<UTC date>`. KV increments are not atomic, so concurrent
requests can exceed a limit briefly. A request is counted before its upstream
call and is not refunded if the provider fails. Do not treat these counters as
billing records. Keep license and admin secrets private and rotate the admin
token if it is exposed.

Photo transcription uses the photo allowance; topic previews use the
smart-model allowance; answer marking uses the marked-answer allowance; other
AI calls use the tutor allowance. Gemini is preferred for photo and
smart-model tasks, with DeepSeek as fallback; DeepSeek is preferred for tutor
and marking tasks, with Gemini as fallback. Managed previews do not enable web
search grounding.

The Worker forwards prompts and photos to the configured AI provider. It
verifies the signed activation payload but cannot verify that the request
comes from the device bound to it. Do not represent this as server-side device
attestation.

Run `npm ci`, `npm test`, and `npm run typecheck` from this directory.
