const MAX_BODY_BYTES = 3 * 1024 * 1024;
const MAX_TEXT_CHARS = 100_000;
const USAGE_TTL_SECONDS = 2 * 24 * 60 * 60;
const BASE32 = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const NAME_OFFSET = 42;
const SIGNATURE_LENGTH = 64;

export type Task = "photo" | "tutor" | "smart" | "mark";
export type PlanId = "free" | "2000" | "5000" | "10000";

export interface Limits {
  photoPages: number;
  tutorMessages: number;
  smartModelMessages: number;
  markedAnswers: number;
}

export interface ManagedAiStatus {
  enabled: boolean;
  plan: PlanId;
  date: string;
  resetAt: string;
  used: Limits;
  limits: Limits;
}

export interface UsageKv {
  get(key: string): Promise<string | null>;
  put(key: string, value: string, options?: { expirationTtl?: number }): Promise<void>;
}

export interface Env {
  USAGE: UsageKv;
  STUDYBITES_LICENSE_PUBKEY?: string;
  ADMIN_TOKEN?: string;
  GEMINI_API_KEY?: string;
  DEEPSEEK_API_KEY?: string;
}

interface LicensePayload {
  licenseId: string;
  deviceTag: string;
  issuedDays: number;
  features: number;
  buyerName: string;
}

interface ChatMessage {
  role: "system" | "user";
  content: string | Array<{ type: "text"; text: string } | { type: "image_url"; image_url: { url: string } }>;
}

interface AiRequest {
  task: Task;
  messages: [ChatMessage, ChatMessage];
  temperature: number;
}

const PLAN_LIMITS: Record<PlanId, Limits> = {
  free: {
    photoPages: 3,
    tutorMessages: 5,
    smartModelMessages: 0,
    markedAnswers: 3,
  },
  "2000": {
    photoPages: 6,
    tutorMessages: 20,
    smartModelMessages: 0,
    markedAnswers: 10,
  },
  "5000": {
    photoPages: 12,
    tutorMessages: 40,
    smartModelMessages: 5,
    markedAnswers: 15,
  },
  "10000": {
    photoPages: 25,
    tutorMessages: 60,
    smartModelMessages: 15,
    markedAnswers: 25,
  },
};

const TASK_LIMIT: Record<Task, keyof Limits> = {
  photo: "photoPages",
  tutor: "tutorMessages",
  smart: "smartModelMessages",
  mark: "markedAnswers",
};

const emptyUsage = (): Limits => ({
  photoPages: 0,
  tutorMessages: 0,
  smartModelMessages: 0,
  markedAnswers: 0,
});

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

function jsonResponse(body: unknown, status: number): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: {
      "content-type": "application/json; charset=utf-8",
      "cache-control": "no-store",
    },
  });
}

function errorResponse(status: number, code: string, message: string, resetAt?: string): Response {
  return jsonResponse(
    { error: { code, message, ...(resetAt ? { resetAt } : {}) } },
    status,
  );
}

function rustWhitespace(value: string): boolean {
  return /^\p{White_Space}$/u.test(value);
}

export function decodeActivationCode(code: string): Uint8Array | null {
  const bytes: number[] = [];
  let buffer = 0;
  let bits = 0;
  for (const raw of code) {
    if (raw === "-" || rustWhitespace(raw)) continue;
    let character = raw.toUpperCase();
    if (character === "O") character = "0";
    if (character === "I" || character === "L") character = "1";
    if (character === "U") return null;
    const value = BASE32.indexOf(character);
    if (value < 0) return null;
    buffer = ((buffer << 5) | value) & 0xffff;
    bits += 5;
    if (bits >= 8) {
      bits -= 8;
      bytes.push((buffer >> bits) & 0xff);
    }
  }
  if (bits >= 5 || (buffer & ((1 << bits) - 1)) !== 0) return null;
  return Uint8Array.from(bytes);
}

function decodeHex(value: string | undefined, length: number): Uint8Array | null {
  if (!value || value.length !== length * 2 || !/^[0-9a-f]+$/i.test(value)) return null;
  const bytes = new Uint8Array(length);
  for (let index = 0; index < length; index += 1) {
    bytes[index] = Number.parseInt(value.slice(index * 2, index * 2 + 2), 16);
  }
  return bytes;
}

function readU32LE(bytes: Uint8Array, offset: number): number {
  return new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength).getUint32(offset, true);
}

function readU64LE(bytes: Uint8Array, offset: number): string {
  let value = 0n;
  for (let index = 7; index >= 0; index -= 1) {
    value = (value << 8n) | BigInt(bytes[offset + index]);
  }
  return value.toString(10);
}

function arrayBuffer(bytes: Uint8Array): ArrayBuffer {
  const copy = new ArrayBuffer(bytes.byteLength);
  new Uint8Array(copy).set(bytes);
  return copy;
}

export async function verifyActivationCode(
  code: string,
  publicKeyHex: string | undefined,
): Promise<LicensePayload | null> {
  const publicKey = decodeHex(publicKeyHex, 32);
  if (!publicKey) throw new Error("The managed AI license verification key is not configured.");
  const bytes = decodeActivationCode(code);
  if (
    !bytes ||
    bytes.length < NAME_OFFSET + SIGNATURE_LENGTH ||
    bytes[0] !== 0x53 ||
    bytes[1] !== 0x41 ||
    bytes[2] !== 1
  ) {
    return null;
  }
  const nameLength = bytes[NAME_OFFSET - 1];
  const bodyLength = NAME_OFFSET + nameLength;
  if (bytes.length < bodyLength + SIGNATURE_LENGTH) return null;

  let buyerName: string;
  try {
    buyerName = new TextDecoder("utf-8", { fatal: true }).decode(bytes.slice(NAME_OFFSET, bodyLength));
  } catch {
    return null;
  }

  let key: CryptoKey;
  try {
    key = await crypto.subtle.importKey(
      "raw",
      arrayBuffer(publicKey),
      { name: "Ed25519" },
      false,
      ["verify"],
    );
  } catch {
    throw new Error("Ed25519 license verification is unavailable in this Worker runtime.");
  }
  let valid: boolean;
  try {
    valid = await crypto.subtle.verify(
      { name: "Ed25519" },
      key,
      arrayBuffer(bytes.slice(bodyLength, bodyLength + SIGNATURE_LENGTH)),
      arrayBuffer(bytes.slice(0, bodyLength)),
    );
  } catch {
    throw new Error("Ed25519 license verification failed.");
  }
  if (!valid) return null;

  return {
    licenseId: readU64LE(bytes, 3),
    deviceTag: Array.from(bytes.slice(11, 19), (byte) => byte.toString(16).padStart(2, "0")).join(""),
    issuedDays: readU32LE(bytes, 19),
    features: bytes[23] | (bytes[24] << 8),
    buyerName,
  };
}

function readBearer(request: Request): string | null {
  const authorization = request.headers.get("Authorization");
  if (!authorization?.startsWith("Bearer ")) return null;
  const value = authorization.slice(7).trim();
  return value || null;
}

function equalSecret(actual: string, expected: string): boolean {
  const encoder = new TextEncoder();
  const left = encoder.encode(actual);
  const right = encoder.encode(expected);
  let difference = left.length ^ right.length;
  const length = Math.max(left.length, right.length);
  for (let index = 0; index < length; index += 1) {
    difference |= (left[index] ?? 0) ^ (right[index] ?? 0);
  }
  return difference === 0;
}

function utcDate(now: Date): string {
  return now.toISOString().slice(0, 10);
}

function nextUtcMidnight(now: Date): string {
  const next = new Date(now);
  next.setUTCHours(24, 0, 0, 0);
  return next.toISOString();
}

function parseLimits(raw: string | null): Limits {
  if (raw === null) return emptyUsage();
  const value: unknown = JSON.parse(raw);
  if (!isRecord(value)) throw new Error("Usage data is invalid.");
  const usage = emptyUsage();
  for (const key of Object.keys(usage) as Array<keyof Limits>) {
    const count = value[key];
    if (typeof count !== "number" || !Number.isSafeInteger(count) || count < 0) {
      throw new Error("Usage data is invalid.");
    }
    usage[key] = count;
  }
  return usage;
}

function planId(value: unknown): PlanId | null {
  return value === "free" || value === "2000" || value === "5000" || value === "10000"
    ? value
    : null;
}

async function readPlan(kv: UsageKv, licenseId: string): Promise<PlanId> {
  const raw = await kv.get(`plan:${licenseId}`);
  if (raw === null) return "free";
  const value: unknown = JSON.parse(raw);
  const parsed = isRecord(value) ? planId(value.plan) : null;
  if (!parsed) throw new Error("Plan data is invalid.");
  return parsed;
}

async function readUsage(kv: UsageKv, licenseId: string, date: string): Promise<Limits> {
  return parseLimits(await kv.get(`usage:${licenseId}:${date}`));
}

function taskFrom(value: unknown): Task | null {
  return value === "photo" || value === "tutor" || value === "smart" || value === "mark"
    ? value
    : null;
}

function validImageUrl(value: unknown): value is string {
  if (typeof value !== "string" || value.length > 2 * 1024 * 1024) return false;
  const match = value.match(/^data:(image\/(?:jpeg|png));base64,([A-Za-z0-9+/]+={0,2})$/);
  if (!match) return false;
  try {
    const binary = atob(match[2]);
    if (btoa(binary) !== match[2]) return false;
    const bytes = Uint8Array.from(binary, (character) => character.charCodeAt(0));
    if (match[1] === "image/jpeg") {
      return bytes.length >= 3 && bytes[0] === 0xff && bytes[1] === 0xd8 && bytes[2] === 0xff;
    }
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
  } catch {
    return false;
  }
}

function validContent(value: unknown): value is ChatMessage["content"] {
  if (typeof value === "string") return value.length <= MAX_TEXT_CHARS;
  if (!Array.isArray(value) || value.length < 1 || value.length > 2) return false;
  return value.every((part) => {
    if (!isRecord(part)) return false;
    if (part.type === "text") return typeof part.text === "string" && part.text.length <= MAX_TEXT_CHARS;
    return (
      part.type === "image_url" &&
      isRecord(part.image_url) &&
      validImageUrl(part.image_url.url)
    );
  });
}

function validateAiRequest(value: unknown): AiRequest | null {
  if (!isRecord(value)) return null;
  const task = taskFrom(value.task);
  const messages = value.messages;
  if (!task || !Array.isArray(messages) || messages.length !== 2) return null;
  const system = messages[0];
  const user = messages[1];
  if (
    !isRecord(system) ||
    system.role !== "system" ||
    typeof system.content !== "string" ||
    system.content.length > MAX_TEXT_CHARS ||
    !isRecord(user) ||
    user.role !== "user" ||
    !validContent(user.content)
  ) {
    return null;
  }
  if (task === "photo" && (!Array.isArray(user.content) || !user.content.some(
    (part) => isRecord(part) && part.type === "image_url",
  ))) return null;
  if (task !== "photo" && Array.isArray(user.content)) return null;
  const temperature =
    typeof value.temperature === "number" && Number.isFinite(value.temperature)
      ? Math.min(1, Math.max(0, value.temperature))
      : 0.2;
  return {
    task,
    messages: [
      { role: "system", content: system.content },
      { role: "user", content: user.content },
    ],
    temperature,
  };
}

async function readJson(request: Request): Promise<unknown | null> {
  const contentType = request.headers.get("Content-Type")?.toLowerCase() ?? "";
  if (!contentType.includes("application/json")) return null;
  const length = Number(request.headers.get("Content-Length") ?? "0");
  if (Number.isFinite(length) && length > MAX_BODY_BYTES) return null;
  const reader = request.body?.getReader();
  if (!reader) return null;
  const chunks: Uint8Array[] = [];
  let size = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    size += value.byteLength;
    if (size > MAX_BODY_BYTES) {
      await reader.cancel();
      return null;
    }
    chunks.push(value);
  }
  const bytes = new Uint8Array(size);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.byteLength;
  }
  try {
    return JSON.parse(new TextDecoder().decode(bytes)) as unknown;
  } catch {
    return null;
  }
}

function imageParts(content: ChatMessage["content"]): Array<Record<string, unknown>> | null {
  if (typeof content === "string") return [{ text: content }];
  const parts: Array<Record<string, unknown>> = [];
  for (const item of content) {
    if (item.type === "text") {
      parts.push({ text: item.text });
      continue;
    }
    const match = item.image_url.url.match(/^data:(image\/(?:jpeg|png|webp));base64,(.+)$/);
    if (!match) return null;
    parts.push({ inline_data: { mime_type: match[1], data: match[2] } });
  }
  return parts;
}

function modelFor(provider: "gemini" | "deepseek", task: Task): string {
  if (provider === "gemini") return task === "smart" ? "gemini-2.5-flash" : "gemini-2.5-flash-lite";
  return task === "smart" ? "deepseek-v4-pro" : "deepseek-flash";
}

async function callGemini(
  request: AiRequest,
  apiKey: string,
  fetcher: typeof fetch,
): Promise<Response> {
  const system = request.messages[0].content as string;
  const parts = imageParts(request.messages[1].content);
  if (!parts) return errorResponse(400, "invalid_image", "The image format is not supported.");
  const model = modelFor("gemini", request.task);
  return fetcher(
    `https://generativelanguage.googleapis.com/v1beta/models/${model}:generateContent`,
    {
      method: "POST",
      headers: { "Content-Type": "application/json", "x-goog-api-key": apiKey },
      body: JSON.stringify({
        systemInstruction: { parts: [{ text: system }] },
        contents: [{ role: "user", parts }],
        generationConfig: { temperature: request.temperature, responseMimeType: "application/json" },
      }),
    },
  );
}

async function callDeepseek(
  request: AiRequest,
  apiKey: string,
  fetcher: typeof fetch,
): Promise<Response> {
  return fetcher("https://api.deepseek.com/chat/completions", {
    method: "POST",
    headers: {
      Authorization: `Bearer ${apiKey}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify({
      model: modelFor("deepseek", request.task),
      messages: request.messages,
      temperature: request.temperature,
      response_format: { type: "json_object" },
      thinking: { type: "disabled" },
    }),
  });
}

async function responseAsOpenAi(response: Response, provider: "gemini" | "deepseek"): Promise<unknown> {
  const raw: unknown = await response.json();
  if (!isRecord(raw)) throw new Error("The AI provider returned an invalid response.");
  if (provider === "deepseek") {
    if (!Array.isArray(raw.choices) || !isRecord(raw.choices[0])) {
      throw new Error("The AI provider returned no answer.");
    }
    return raw;
  }
  const candidates = raw.candidates;
  if (!Array.isArray(candidates) || !isRecord(candidates[0])) {
    throw new Error("The AI provider returned no answer.");
  }
  const content = candidates[0].content;
  if (!isRecord(content) || !Array.isArray(content.parts)) {
    throw new Error("The AI provider returned no answer.");
  }
  const answer = content.parts
    .filter(isRecord)
    .map((part) => (typeof part.text === "string" ? part.text : ""))
    .join("");
  if (!answer) throw new Error("The AI provider returned no answer.");
  return { choices: [{ message: { content: answer } }] };
}

async function callProvider(
  request: AiRequest,
  provider: "gemini" | "deepseek",
  env: Env,
  fetcher: typeof fetch,
): Promise<Response> {
  const key = provider === "gemini" ? env.GEMINI_API_KEY : env.DEEPSEEK_API_KEY;
  if (!key?.trim()) return errorResponse(503, "provider_unavailable", "No managed AI provider is configured.");
  return provider === "gemini"
    ? callGemini(request, key, fetcher)
    : callDeepseek(request, key, fetcher);
}

function providerOrder(task: Task): Array<"gemini" | "deepseek"> {
  return task === "photo" || task === "smart"
    ? ["gemini", "deepseek"]
    : ["deepseek", "gemini"];
}

async function runAi(
  request: AiRequest,
  env: Env,
  fetcher: typeof fetch,
): Promise<Response> {
  let anyProviderConfigured = false;
  for (const provider of providerOrder(request.task)) {
    const key = provider === "gemini" ? env.GEMINI_API_KEY : env.DEEPSEEK_API_KEY;
    if (!key?.trim()) continue;
    anyProviderConfigured = true;
    let response: Response;
    try {
      response = await callProvider(request, provider, env, fetcher);
    } catch {
      continue;
    }
    if (!response.ok) continue;
    try {
      return jsonResponse(await responseAsOpenAi(response, provider), 200);
    } catch {
      continue;
    }
  }
  return anyProviderConfigured
    ? errorResponse(502, "upstream_unavailable", "Managed AI is temporarily unavailable.")
    : errorResponse(503, "provider_unavailable", "Managed AI is not configured yet.");
}

async function authenticateLicense(
  request: Request,
  env: Env,
): Promise<LicensePayload | Response> {
  const code = readBearer(request);
  if (!code) return errorResponse(401, "invalid_license", "A valid activation code is required.");
  try {
    const license = await verifyActivationCode(code, env.STUDYBITES_LICENSE_PUBKEY);
    return license ?? errorResponse(401, "invalid_license", "The activation code is not valid.");
  } catch {
    return errorResponse(503, "license_verification_unavailable", "License verification is unavailable.");
  }
}

async function statusFor(
  env: Env,
  licenseId: string,
  now: Date,
): Promise<ManagedAiStatus> {
  const date = utcDate(now);
  const [plan, used] = await Promise.all([
    readPlan(env.USAGE, licenseId),
    readUsage(env.USAGE, licenseId, date),
  ]);
  return {
    enabled: Boolean(env.GEMINI_API_KEY?.trim() || env.DEEPSEEK_API_KEY?.trim()),
    plan,
    date,
    resetAt: nextUtcMidnight(now),
    used,
    limits: PLAN_LIMITS[plan],
  };
}

async function consume(
  env: Env,
  licenseId: string,
  task: Task,
  now: Date,
): Promise<{ status: "allowed"; plan: PlanId } | { status: "limited"; resetAt: string }> {
  const date = utcDate(now);
  const plan = await readPlan(env.USAGE, licenseId);
  const used = await readUsage(env.USAGE, licenseId, date);
  const bucket = TASK_LIMIT[task];
  if (used[bucket] >= PLAN_LIMITS[plan][bucket]) {
    return { status: "limited", resetAt: nextUtcMidnight(now) };
  }
  used[bucket] += 1;
  await env.USAGE.put(`usage:${licenseId}:${date}`, JSON.stringify(used), {
    expirationTtl: USAGE_TTL_SECONDS,
  });
  return { status: "allowed", plan };
}

export function createWorker(
  fetcher: typeof fetch = fetch,
  clock: () => Date = () => new Date(),
) {
  return {
    async fetch(request: Request, env: Env): Promise<Response> {
      const url = new URL(request.url);
      if (url.pathname === "/v1/status") {
        if (request.method !== "GET") {
          return errorResponse(405, "method_not_allowed", "Use GET to check managed AI status.");
        }
        const license = await authenticateLicense(request, env);
        if (license instanceof Response) return license;
        try {
          return jsonResponse(await statusFor(env, license.licenseId, clock()), 200);
        } catch {
          return errorResponse(503, "status_unavailable", "Managed AI usage is temporarily unavailable.");
        }
      }

      if (url.pathname === "/v1/ai") {
        if (request.method !== "POST") {
          return errorResponse(405, "method_not_allowed", "Use POST to request managed AI.");
        }
        const license = await authenticateLicense(request, env);
        if (license instanceof Response) return license;
        const raw = await readJson(request);
        if (raw === null) return errorResponse(400, "invalid_request", "Send a valid, bounded JSON request.");
        const input = validateAiRequest(raw);
        if (!input) return errorResponse(400, "invalid_request", "The AI task or messages are not valid.");
        if (!env.GEMINI_API_KEY?.trim() && !env.DEEPSEEK_API_KEY?.trim()) {
          return errorResponse(503, "provider_unavailable", "Managed AI is not configured yet.");
        }
        let consumed: Awaited<ReturnType<typeof consume>>;
        try {
          consumed = await consume(env, license.licenseId, input.task, clock());
        } catch {
          return errorResponse(503, "usage_unavailable", "Managed AI limits are temporarily unavailable.");
        }
        if (consumed.status === "limited") {
          return errorResponse(
            429,
            "quota_exceeded",
            "Today's managed AI limit has been reached. It resets at midnight UTC.",
            consumed.resetAt,
          );
        }
        return runAi(input, env, fetcher);
      }

      if (url.pathname === "/admin/plan") {
        if (request.method !== "POST") {
          return errorResponse(405, "method_not_allowed", "Use POST to update a plan.");
        }
        const expected = env.ADMIN_TOKEN;
        const actual = readBearer(request);
        if (!expected?.trim() || !actual || !equalSecret(actual, expected)) {
          return errorResponse(401, "unauthorized", "Administrator authorization is required.");
        }
        const raw = await readJson(request);
        if (!isRecord(raw) || typeof raw.licenseId !== "string" || !/^\d{1,20}$/.test(raw.licenseId)) {
          return errorResponse(400, "invalid_license_id", "Enter a decimal license ID.");
        }
        const licenseId = BigInt(raw.licenseId);
        if (licenseId > 0xffff_ffff_ffff_ffffn) {
          return errorResponse(400, "invalid_license_id", "The license ID is outside the supported range.");
        }
        const plan = planId(raw.plan);
        if (!plan) return errorResponse(400, "invalid_plan", "Choose free, 2000, 5000 or 10000.");
        try {
          await env.USAGE.put(
            `plan:${licenseId.toString(10)}`,
            JSON.stringify({ plan, updatedAt: clock().toISOString() }),
          );
        } catch {
          return errorResponse(503, "plan_unavailable", "The plan could not be saved.");
        }
        return jsonResponse({ ok: true, licenseId: licenseId.toString(10), plan }, 200);
      }

      return errorResponse(404, "not_found", "Not found.");
    },
  };
}

export default createWorker();
