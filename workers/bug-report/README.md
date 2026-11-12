# Bug report Worker

The Worker sends student-submitted bug reports to the Resend account owner's
inbox. It does not log or persist report content. Daily limits and seven-day
deduplication keys use the `RATE` KV namespace.

## One-time setup

1. Create a Resend account and API key with email-send access. Resend's test
   sender, `onboarding@resend.dev`, can deliver only to the account owner's
   email; set that address as `BUG_REPORT_TO`. This is for testing only. Before
   public launch, use a verified-domain sender or Cloudflare Email Service and
   update the Worker sender configuration.
2. Set up a Cloudflare account with Workers enabled. Wrangler automatically
   provisions the `RATE` KV namespace on the first deploy and saves its binding
   in the Cloudflare Worker configuration.
3. Add these GitHub Actions secrets:
   - `CLOUDFLARE_API_TOKEN` with Workers edit permission
   - `CLOUDFLARE_ACCOUNT_ID`
   - `RESEND_API_KEY`
   - `BUG_REPORT_TO` (the email address associated with the Resend account)
4. Run **Deploy bug report Worker** from Actions, or push a change to
   `workers/bug-report/` on `main`. The workflow tests first, deploys, and
   prints the Worker URL in its summary.
5. Add the deployed HTTPS root URL as the repository variable
   `STUDYBITES_BUG_REPORT_URL`. Windows and Android builds compile it in; when
   it is empty, the app offers only **Copy report**.
6. To enable a browser client later, set the Worker variable `ALLOWED_ORIGINS`
   to an exact comma-separated origin allowlist. The native app does not need
   CORS.

The free Resend plan currently allows 100 messages per day and 3,000 per month.
The Worker caps sending at 60 per UTC day globally, plus 5 per device and 10
per source IP. Cloudflare KV counters are eventually consistent, so concurrent
requests may briefly race; Resend's idempotency key prevents duplicate delivery
for retries of the same report ID.

Run `npm ci`, `npm test`, and `npm run typecheck` from this directory.
