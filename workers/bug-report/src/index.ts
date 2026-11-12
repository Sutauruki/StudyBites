const MAX_BODY_BYTES = 4 * 1024 * 1024;
const MAX_ATTACHMENT_BYTES = Math.floor(1.2 * 1024 * 1024);
const MAX_DIAGNOSTICS_BYTES = 4 * 1024;
const DIAGNOSTIC_KEYS = new Set([
  "appVersion",
  "platform",
  "webview",
  "screenSize",
  "language",
  "timeZone",
  "online",
  "view",
  "licenseState",
  "recentErrors",
]);

export interface RateKv {
  get(key: string): Promise<string | null>;
  put(key: string, value: string, options?: { expirationTtl?: number }): Promise<void>;
}

export interface Env {
  RATE: RateKv;
  RESEND_API_KEY: string;
  BUG_REPORT_TO: string;
  ALLOWED_ORIGINS?: string;
}

export interface Attachment {
  filename: string;
  mime: "image/jpeg" | "image/png" | "image/webp";
  data: string;
}

export interface BugReport {
  id: string;
  deviceId: string;
  title: string;
  details: string;
  steps?: string;
  contact?: string;
  category: "bug" | "confusing" | "idea";
  diag: Record<string, string>;
  attachments: Attachment[];
}

const utf8Length = (value: string) => new TextEncoder().encode(value).length;

function stripControls(value: string): string {
  return value.replace(/[\u0000-\u0008\u000B-\u000D\u000E-\u001F\u007F]/g, "");
}

function redactDiagnostic(value: string): string {
  return stripControls(value)
    .replace(/\bBearer\s+[A-Za-z0-9._~+/-]+=*/gi, "Bearer [redacted]")
    .replace(/\b(?:sk-[A-Za-z0-9_-]{12,}|AIza[A-Za-z0-9_-]{20,})\b/gi, "[redacted]")
    .replace(
      /\b(api[_-]?key|token|password|secret|authorization)\s*[:=]\s*["']?[^,\s"';&]+/gi,
      "$1=[redacted]",
    )
    .replace(
      /(?:[A-Za-z]:[\\/]|\/(?:home|Users|workspaces|tmp|var|private|data|mnt|app)\/)[^\r\n]*/gi,
      "[path]",
    )
    .replace(/\b[A-Za-z0-9+/_=-]{80,}\b/g, "[redacted]");
}

export function escapeHtml(value: string): string {
  return stripControls(value)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;")
    .replace(/\r\n|\r|\n/g, "<br>");
}

function isUuid(value: unknown): value is string {
  return (
    typeof value === "string" &&
    /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(
      value,
    )
  );
}

function text(value: unknown, min: number, max: number): string | null {
  if (typeof value !== "string") return null;
  const clean = stripControls(value).trim();
  const length = clean.length;
  return length >= min && length <= max ? clean : null;
}

function decodeBase64(value: string): Uint8Array | null {
  if (!/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(value)) {
    return null;
  }
  try {
    const binary = atob(value);
    if (btoa(binary) !== value) return null;
    return Uint8Array.from(binary, (character) => character.charCodeAt(0));
  } catch {
    return null;
  }
}

function matchesMagic(mime: Attachment["mime"], bytes: Uint8Array): boolean {
  if (mime === "image/jpeg") {
    return bytes.length >= 3 && bytes[0] === 0xff && bytes[1] === 0xd8 && bytes[2] === 0xff;
  }
  if (mime === "image/png") {
    return (
      bytes.length >= 8 &&
      bytes[0] === 0x89 &&
      bytes[1] === 0x50 &&
      bytes[2] === 0x4e &&
      bytes[3] === 0x47 &&
      bytes[4] === 0x0d &&
      bytes[5] === 0x0a &&
      bytes[6] === 0x1a &&
      bytes[7] === 0x0a
    );
  }
  return (
    bytes.length >= 12 &&
    String.fromCharCode(...bytes.slice(0, 4)) === "RIFF" &&
    String.fromCharCode(...bytes.slice(8, 12)) === "WEBP"
  );
}

type Validation = { report: BugReport } | { error: string };

export function validateReport(input: unknown): Validation {
  if (!input || typeof input !== "object" || Array.isArray(input)) {
    return { error: "The report is not valid JSON." };
  }
  const raw = input as Record<string, unknown>;
  if (!isUuid(raw.id) || !isUuid(raw.deviceId)) {
    return { error: "The report identifier is not valid." };
  }
  const title = text(raw.title, 1, 120);
  const details = text(raw.details, 1, 2000);
  const steps =
    raw.steps === undefined || raw.steps === null || raw.steps === ""
      ? undefined
      : text(raw.steps, 1, 1500);
  const contact =
    raw.contact === undefined || raw.contact === null || raw.contact === ""
      ? undefined
      : text(raw.contact, 1, 120);
  if (!title) return { error: "Add a summary of 1 to 120 characters." };
  if (!details) return { error: "Describe what happened in 1 to 2,000 characters." };
  if (raw.steps !== undefined && raw.steps !== null && raw.steps !== "" && !steps) {
    return { error: "Steps must be 1 to 1,500 characters." };
  }
  if (raw.contact !== undefined && raw.contact !== null && raw.contact !== "" && !contact) {
    return { error: "Contact email must be 1 to 120 characters." };
  }
  if (contact && !/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(contact)) {
    return { error: "Enter a valid contact email address." };
  }
  if (raw.category !== "bug" && raw.category !== "confusing" && raw.category !== "idea") {
    return { error: "Choose Bug, Confusing or Idea." };
  }

  const rawDiag = raw.diag ?? {};
  if (!rawDiag || typeof rawDiag !== "object" || Array.isArray(rawDiag)) {
    return { error: "Technical details are not valid." };
  }
  const diag: Record<string, string> = {};
  let diagBytes = 0;
  const diagEntries = Object.entries(rawDiag as Record<string, unknown>);
  if (diagEntries.length > 32) return { error: "Technical details are too long." };
  for (const [key, value] of diagEntries) {
    const cleanKey = stripControls(key).trim();
    if (!DIAGNOSTIC_KEYS.has(cleanKey) || cleanKey.length > 80 || typeof value !== "string") {
      return { error: "Technical details must contain short text values." };
    }
    const cleanValue = redactDiagnostic(value).slice(0, 512);
    diagBytes += utf8Length(cleanKey) + utf8Length(cleanValue);
    if (diagBytes > MAX_DIAGNOSTICS_BYTES) {
      return { error: "Technical details exceed the 4 KB limit." };
    }
    diag[cleanKey] = cleanValue;
  }

  if (!Array.isArray(raw.attachments) || raw.attachments.length > 3) {
    return { error: "Choose no more than 3 screenshots." };
  }
  const attachments: Attachment[] = [];
  for (const item of raw.attachments) {
    if (!item || typeof item !== "object" || Array.isArray(item)) {
      return { error: "A screenshot is not valid." };
    }
    const value = item as Record<string, unknown>;
    if (
      typeof value.filename !== "string" ||
      typeof value.mime !== "string" ||
      typeof value.data !== "string" ||
      !["image/jpeg", "image/png", "image/webp"].includes(value.mime)
    ) {
      return { error: "Screenshots must be JPEG, PNG or WebP images." };
    }
    const filename = stripControls(value.filename)
      .replace(/[\\/]/g, "_")
      .trim()
      .slice(0, 120);
    const bytes = decodeBase64(value.data);
    if (!filename || !bytes || bytes.length > MAX_ATTACHMENT_BYTES) {
      return { error: "A screenshot is invalid or larger than 1.2 MB." };
    }
    const mime = value.mime as Attachment["mime"];
    if (!matchesMagic(mime, bytes)) {
      return { error: "A screenshot's file type does not match its contents." };
    }
    attachments.push({ filename, mime, data: value.data });
  }

  return {
    report: {
      id: raw.id,
      deviceId: raw.deviceId,
      title,
      details,
      ...(steps ? { steps } : {}),
      ...(contact ? { contact } : {}),
      category: raw.category,
      diag,
      attachments,
    },
  };
}

function jsonResponse(
  body: Record<string, unknown>,
  status: number,
  origin?: string,
): Response {
  const headers = new Headers({
    "content-type": "application/json; charset=utf-8",
    "cache-control": "no-store",
  });
  if (origin) {
    headers.set("access-control-allow-origin", origin);
    headers.set("access-control-allow-methods", "POST, OPTIONS");
    headers.set("access-control-allow-headers", "Content-Type");
    headers.set("vary", "Origin");
  }
  return new Response(status === 204 ? null : JSON.stringify(body), { status, headers });
}

async function readLimitedBody(request: Request, limit: number): Promise<Uint8Array | null> {
  const reader = request.body?.getReader();
  if (!reader) return new Uint8Array();
  const chunks: Uint8Array[] = [];
  let length = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    length += value.byteLength;
    if (length > limit) {
      await reader.cancel();
      return null;
    }
    chunks.push(value);
  }
  const body = new Uint8Array(length);
  let offset = 0;
  for (const chunk of chunks) {
    body.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return body;
}

function originAllowed(request: Request, env: Env): string | undefined | null {
  const origin = request.headers.get("Origin");
  if (!origin) return undefined;
  const allowed = (env.ALLOWED_ORIGINS ?? "")
    .split(",")
    .map((value) => value.trim())
    .filter(Boolean);
  return allowed.includes(origin) ? origin : null;
}

async function hashRateValue(value: string): Promise<string> {
  const bytes = new TextEncoder().encode(value);
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
  return Array.from(digest, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

async function count(kv: RateKv, key: string): Promise<number> {
  const value = await kv.get(key);
  if (value === null) return 0;
  const parsed = Number(value);
  return Number.isSafeInteger(parsed) && parsed >= 0 ? parsed : 0;
}

async function increment(kv: RateKv, key: string, value: number): Promise<void> {
  await kv.put(key, String(value + 1), { expirationTtl: 2 * 24 * 60 * 60 });
}

function htmlReport(report: BugReport): string {
  const diagnosticRows = Object.entries(report.diag)
    .map(([key, value]) => `<li><strong>${escapeHtml(key)}:</strong> ${escapeHtml(value)}</li>`)
    .join("");
  return [
    `<h2>${escapeHtml(report.title)}</h2>`,
    `<p><strong>Category:</strong> ${escapeHtml(report.category)}</p>`,
    `<h3>What happened</h3><p>${escapeHtml(report.details)}</p>`,
    report.steps ? `<h3>Steps</h3><p>${escapeHtml(report.steps)}</p>` : "",
    report.contact ? `<p><strong>Contact:</strong> ${escapeHtml(report.contact)}</p>` : "",
    diagnosticRows ? `<h3>Device details</h3><ul>${diagnosticRows}</ul>` : "",
    `<p><strong>Report ID:</strong> ${escapeHtml(report.id)}</p>`,
  ].join("\n");
}

function plainReport(report: BugReport): string {
  const diagnostics = Object.entries(report.diag)
    .map(([key, value]) => `${key}: ${value}`)
    .join("\n");
  return [
    `Category: ${report.category}`,
    `Summary: ${report.title}`,
    "",
    "What happened:",
    report.details,
    report.steps ? `\nSteps:\n${report.steps}` : "",
    report.contact ? `\nContact: ${report.contact}` : "",
    diagnostics ? `\nDevice details:\n${diagnostics}` : "",
    `\nReport ID: ${report.id}`,
  ].join("\n");
}

function subjectPart(value: string, maxLength: number): string {
  return stripControls(value).replace(/[\r\n]/g, " ").slice(0, maxLength);
}

export function createWorker(sendFetch: typeof fetch = fetch) {
  return {
    async fetch(request: Request, env: Env): Promise<Response> {
      const origin = originAllowed(request, env);
      if (origin === null) return jsonResponse({ ok: false, error: "Origin is not allowed." }, 403);
      if (request.method === "OPTIONS") {
        return origin
          ? jsonResponse({ ok: true }, 204, origin)
          : jsonResponse({ ok: false, error: "Origin is not allowed." }, 403);
      }
      const url = new URL(request.url);
      if (url.pathname !== "/") {
        return jsonResponse({ ok: false, error: "Not found." }, 404, origin);
      }
      if (request.method !== "POST") {
        return jsonResponse({ ok: false, error: "Use POST to submit a report." }, 405, origin);
      }
      const contentLength = Number(request.headers.get("Content-Length") ?? "0");
      if (Number.isFinite(contentLength) && contentLength > MAX_BODY_BYTES) {
        return jsonResponse({ ok: false, error: "The report is larger than 4 MB." }, 413, origin);
      }
      if (!request.headers.get("Content-Type")?.toLowerCase().includes("application/json")) {
        return jsonResponse({ ok: false, error: "Send the report as JSON." }, 415, origin);
      }

      let rawBody: Uint8Array | null;
      try {
        rawBody = await readLimitedBody(request, MAX_BODY_BYTES);
      } catch {
        return jsonResponse({ ok: false, error: "The report body could not be read." }, 400, origin);
      }
      if (!rawBody) {
        return jsonResponse({ ok: false, error: "The report is larger than 4 MB." }, 413, origin);
      }
      let input: unknown;
      try {
        input = JSON.parse(new TextDecoder().decode(rawBody));
      } catch {
        return jsonResponse({ ok: false, error: "The report is not valid JSON." }, 400, origin);
      }
      const validation = validateReport(input);
      if ("error" in validation) {
        return jsonResponse({ ok: false, error: validation.error }, 400, origin);
      }
      const report = validation.report;

      if (!env.RESEND_API_KEY || !env.BUG_REPORT_TO) {
        return jsonResponse({ ok: false, error: "Bug reports are not configured." }, 503, origin);
      }

      const day = new Date().toISOString().slice(0, 10);
      const reportKey = `report:${report.id}`;
      const ip = request.headers.get("CF-Connecting-IP") ?? "unknown";
      let deviceKey: string;
      let addressKey: string;
      try {
        const [deviceHash, addressHash] = await Promise.all([
          hashRateValue(report.deviceId),
          hashRateValue(ip),
        ]);
        deviceKey = `device:${deviceHash}:${day}`;
        addressKey = `ip:${addressHash}:${day}`;
        if (await env.RATE.get(reportKey)) return jsonResponse({ ok: true }, 200, origin);
        const [deviceCount, addressCount, globalCount] = await Promise.all([
          count(env.RATE, deviceKey),
          count(env.RATE, addressKey),
          count(env.RATE, `global:${day}`),
        ]);
        if (deviceCount >= 5 || addressCount >= 10 || globalCount >= 60) {
          const tomorrow = new Date(`${day}T00:00:00.000Z`);
          tomorrow.setUTCDate(tomorrow.getUTCDate() + 1);
          return jsonResponse(
            {
              ok: false,
              error: "Too many reports today. Please try again tomorrow.",
              resetsAt: tomorrow.toISOString(),
            },
            429,
            origin,
          );
        }
      } catch {
        return jsonResponse({ ok: false, error: "Report limits are temporarily unavailable." }, 503, origin);
      }

      const platform = subjectPart(report.diag.platform ?? "unknown", 40);
      const appVersion = subjectPart(report.diag.appVersion ?? "unknown", 40);
      const attachments = report.attachments.map(({ filename, data }) => ({
        filename,
        content: data,
      }));
      const payload = {
        from: "StudyBites Bugs <onboarding@resend.dev>",
        to: [env.BUG_REPORT_TO],
        ...(report.contact ? { reply_to: report.contact } : {}),
        subject: `[StudyBites ${report.category} #${report.id.slice(0, 6)}] ${subjectPart(
          report.title,
          120,
        )} (${platform} ${appVersion})`,
        html: htmlReport(report),
        text: plainReport(report),
        ...(attachments.length ? { attachments } : {}),
      };
      let sent: Response;
      try {
        sent = await sendFetch("https://api.resend.com/emails", {
          method: "POST",
          headers: {
            Authorization: `Bearer ${env.RESEND_API_KEY}`,
            "Content-Type": "application/json",
            "Idempotency-Key": report.id,
          },
          body: JSON.stringify(payload),
        });
      } catch {
        return jsonResponse({ ok: false, error: "Email delivery is temporarily unavailable." }, 502, origin);
      }
      if (!sent.ok) {
        return jsonResponse({ ok: false, error: "Email delivery is temporarily unavailable." }, 502, origin);
      }

      try {
        await Promise.all([
          env.RATE.put(reportKey, "sent", { expirationTtl: 7 * 24 * 60 * 60 }),
          increment(env.RATE, deviceKey, (await count(env.RATE, deviceKey))),
          increment(env.RATE, addressKey, (await count(env.RATE, addressKey))),
          increment(env.RATE, `global:${day}`, (await count(env.RATE, `global:${day}`))),
        ]);
      } catch {
        return jsonResponse({ ok: false, error: "Report receipt could not be saved; retry safely." }, 502, origin);
      }
      return jsonResponse({ ok: true }, 200, origin);
    },
  };
}

export default createWorker();
