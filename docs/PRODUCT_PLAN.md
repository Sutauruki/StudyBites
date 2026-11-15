# StudyBites product plan

## Confirmed products

- Windows and Android apps are offline-first, use a one-time license, and let students paste their own AI keys into the app. An optional managed "StudyBites AI" Worker can serve as the last provider fallback when the build is configured for it. The price is not set yet.
- A later web version remains a separate product using accounts and subscriptions. In-app key pasting stays; the managed AI Worker does not make the desktop/mobile apps account-based.

## Daily limits and cost target

Limits are photo pages / tutor messages / smart-model messages / marked answers:

| Plan | Photo pages | Tutor messages | Smart-model messages | Marked answers | Worst-case monthly AI cost |
| --- | ---: | ---: | ---: | ---: | ---: |
| Free | 3 | 5 | 0 | 3 | about 260 naira |
| 2,000 naira | 6 | 20 | 0 | 10 | about 740 naira |
| 5,000 naira | 12 | 40 | 5 | 15 | about 1,850 naira |
| 10,000 naira | 25 | 60 | 15 | 25 | about 3,730 naira |

These estimates use 1,400 naira per dollar. Flashcard review, CBT practice,
and offline study use no AI and are uncapped.

## AI

- The BYO-key providers are Google Gemini, Groq, OpenRouter, and DeepSeek. The
  existing default order remains Gemini, Groq, OpenRouter, then DeepSeek.
- Gemini 2.5 Flash-Lite is the cost target for photo transcription, notes, normal tutor work, and marking.
- Gemini 2.5 Flash is for harder smart questions; thinking is low.
- DeepSeek V4.1 Flash costs about $0.15 per million cache-miss input tokens and
  $0.60 per million output tokens off-peak, double at peak; cache-hit input is
  cheaper. Gemini stays preferred for photo transcription until handwriting
  quality has been compared.
- Managed plans use a paid key. Model names remain settings because Google retires models.
- A bought key never ships in the app. It lives as a Cloudflare Worker secret, and the app calls the Worker with the activation code.
- The Worker verifies the signed activation payload and applies per-license UTC daily limits. It does not attest the device fingerprint; Cloudflare KV counters are best-effort under concurrent requests and are not billing records.

## Web stack

Cloudflare Pages, Workers, D1, and R2; Better Auth with Google sign-in; Paystack.
PDF text extraction and photo resizing happen in the browser.

## Licensing automation

The app sends a request code and email to a Worker. The Worker creates a Paystack
payment, verifies the signed Paystack webhook (including amount, while ignoring
repeats), and signs the activation code with the private key held as a Worker
secret in the existing code format. The app polls, unlocks, and verifies
offline afterwards. Device moves are limited per email per year.

Free trial means an activation code with an expiry date. Coupons lower the
Paystack amount. A friend gets about 10 percent off; the referrer gets 10
percent of the confirmed payment as credit, capped, with no self-referral and
no stacking with coupons.

## Library and community

Only derived notes and cards are shared, after student review. AI screening and
report/takedown are required. The scheme-of-work library comes first, followed
by a shared notes library derived from notes and cards only, never original
files, with 5,000 naira plans and up, then comment threads. There is no
real-time chat.

## Other confirmed decisions

- Do Not Disturb is Android-only, opt-in, priority-only by default, and always restored afterwards. iOS, Windows, and web do not support it.
- Session state must survive Android killing the app.
- Settings offers opt-in bug reports with a summary, details, and up to three screenshots. The app calls a small Cloudflare Worker, which emails the developer through Resend. No email key ships in the app; reports are rate-limited per device, IP, and day and queued locally while offline. Technical details omit notes, cards, subjects, file names, API keys, and licence codes.
- Nibble recording is last because phone microphones are weak from the back of a hall. The lecturer-consent QR idea is retained for then.
