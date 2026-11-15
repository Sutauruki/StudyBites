import { describe, expect, it, vi } from "vitest";
import fixture from "./fixtures/license.json";
import {
  createWorker,
  decodeActivationCode,
  verifyActivationCode,
  type Env,
  type UsageKv,
} from "../src/index";

class MemoryKv implements UsageKv {
  readonly values = new Map<string, string>();

  async get(key: string): Promise<string | null> {
    return this.values.get(key) ?? null;
  }

  async put(key: string, value: string): Promise<void> {
    this.values.set(key, value);
  }
}

const NOW = new Date("2026-11-12T23:59:30.000Z");
const normalizedCode = fixture.activationCode
  .toLowerCase()
  .replaceAll("-", " ")
  .replaceAll("1", "I")
  .replaceAll("0", "O");

function setup(fetcher: typeof fetch = vi.fn(async () => Response.json({
  choices: [{ message: { content: "ok" } }],
}))) {
  const kv = new MemoryKv();
  const env: Env = {
    USAGE: kv,
    STUDYBITES_LICENSE_PUBKEY: fixture.publicKeyHex,
    ADMIN_TOKEN: "admin-test-token",
    GEMINI_API_KEY: "gemini-secret",
    DEEPSEEK_API_KEY: "deepseek-secret",
  };
  return { worker: createWorker(fetcher, () => new Date(NOW)), env, kv, fetcher };
}

function authorizedRequest(
  path: string,
  method: string,
  body?: unknown,
  token = fixture.activationCode,
): Request {
  return new Request(`https://ai.test${path}`, {
    method,
    headers: {
      Authorization: `Bearer ${token}`,
      ...(body === undefined ? {} : { "Content-Type": "application/json" }),
    },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  });
}

const tutorRequest = () => ({
  task: "tutor",
  messages: [
    { role: "system", content: "Answer in JSON." },
    { role: "user", content: "Explain this idea." },
  ],
  temperature: 0.2,
});

describe("managed AI worker", () => {
  it("verifies the Rust fixture and accepts the same code normalization", async () => {
    const parsed = await verifyActivationCode(
      normalizedCode,
      fixture.publicKeyHex,
    );
    expect(parsed).toMatchObject({
      licenseId: "7",
      deviceTag: fixture.deviceTagHex,
      issuedDays: 20400,
      features: 0,
      buyerName: "Fixture Student",
    });
    expect(decodeActivationCode(normalizedCode)).toEqual(
      decodeActivationCode(fixture.activationCode),
    );
    await expect(
      verifyActivationCode(`${fixture.activationCode}U`, fixture.publicKeyHex),
    ).resolves.toBeNull();
  });

  it("rejects invalid activation codes before reporting or proxying AI", async () => {
    const { worker, env, fetcher } = setup();
    const response = await worker.fetch(
      authorizedRequest("/v1/status", "GET", undefined, fixture.activationCode.replace("A", "B")),
      env,
    );
    expect(response.status).toBe(401);
    await expect(response.json()).resolves.toMatchObject({
      error: { code: "invalid_license" },
    });
    expect(fetcher).not.toHaveBeenCalled();
  });

  it("returns UTC plan limits and current usage without requiring a request body", async () => {
    const { worker, env, kv } = setup();
    await kv.put("usage:7:2026-11-12", JSON.stringify({
      photoPages: 1,
      tutorMessages: 2,
      smartModelMessages: 0,
      markedAnswers: 1,
    }));
    const response = await worker.fetch(authorizedRequest("/v1/status", "GET"), env);
    expect(response.status).toBe(200);
    await expect(response.json()).resolves.toEqual({
      enabled: true,
      plan: "free",
      date: "2026-11-12",
      resetAt: "2026-11-13T00:00:00.000Z",
      used: {
        photoPages: 1,
        tutorMessages: 2,
        smartModelMessages: 0,
        markedAnswers: 1,
      },
      limits: {
        photoPages: 3,
        tutorMessages: 5,
        smartModelMessages: 0,
        markedAnswers: 3,
      },
    });
  });

  it("reports managed AI disabled when no upstream key is configured", async () => {
    const { worker, env } = setup();
    delete env.GEMINI_API_KEY;
    delete env.DEEPSEEK_API_KEY;
    const response = await worker.fetch(authorizedRequest("/v1/status", "GET"), env);
    expect(response.status).toBe(200);
    await expect(response.json()).resolves.toMatchObject({
      enabled: false,
      plan: "free",
    });
  });

  it("does not charge usage when no upstream provider is configured", async () => {
    const { worker, env, kv, fetcher } = setup();
    delete env.GEMINI_API_KEY;
    delete env.DEEPSEEK_API_KEY;
    const response = await worker.fetch(
      authorizedRequest("/v1/ai", "POST", tutorRequest()),
      env,
    );
    expect(response.status).toBe(503);
    expect(kv.values.has("usage:7:2026-11-12")).toBe(false);
    expect(fetcher).not.toHaveBeenCalled();
  });

  it("uses a server provider key and records only the task count", async () => {
    const upstream = vi.fn(async (_url: string | URL | Request, init?: RequestInit) => {
      expect(new Headers(init?.headers).get("Authorization")).toBe("Bearer deepseek-secret");
      const body = JSON.parse(String(init?.body)) as Record<string, unknown>;
      expect(body).not.toHaveProperty("task");
      expect(JSON.stringify(body)).not.toContain(fixture.activationCode);
      return Response.json({ choices: [{ message: { content: "A concise answer." } }] });
    });
    const { worker, env, kv } = setup(upstream);
    const response = await worker.fetch(
      authorizedRequest("/v1/ai", "POST", tutorRequest()),
      env,
    );
    expect(response.status).toBe(200);
    await expect(response.json()).resolves.toMatchObject({
      choices: [{ message: { content: "A concise answer." } }],
    });
    expect(upstream).toHaveBeenCalledTimes(1);
    expect(kv.values.get("usage:7:2026-11-12")).toBe(
      JSON.stringify({
        photoPages: 0,
        tutorMessages: 1,
        smartModelMessages: 0,
        markedAnswers: 0,
      }),
    );
  });

  it("blocks zero-limit smart actions before contacting a provider", async () => {
    const { worker, env, fetcher } = setup();
    const response = await worker.fetch(
      authorizedRequest(
        "/v1/ai",
        "POST",
        { ...tutorRequest(), task: "smart" },
      ),
      env,
    );
    expect(response.status).toBe(429);
    await expect(response.json()).resolves.toMatchObject({
      error: {
        code: "quota_exceeded",
        resetAt: "2026-11-13T00:00:00.000Z",
      },
    });
    expect(fetcher).not.toHaveBeenCalled();
  });

  it("enforces the per-license daily bucket and reports the reset time", async () => {
    const { worker, env, kv, fetcher } = setup();
    await kv.put("usage:7:2026-11-12", JSON.stringify({
      photoPages: 0,
      tutorMessages: 5,
      smartModelMessages: 0,
      markedAnswers: 0,
    }));
    const response = await worker.fetch(
      authorizedRequest("/v1/ai", "POST", tutorRequest()),
      env,
    );
    expect(response.status).toBe(429);
    await expect(response.json()).resolves.toMatchObject({
      error: { code: "quota_exceeded", resetAt: "2026-11-13T00:00:00.000Z" },
    });
    expect(fetcher).not.toHaveBeenCalled();
  });

  it("requires the administrator token and normalizes decimal license IDs", async () => {
    const { worker, env, kv } = setup();
    const body = { licenseId: "0007", plan: "5000" };
    const denied = await worker.fetch(
      authorizedRequest("/admin/plan", "POST", body, "wrong-token"),
      env,
    );
    expect(denied.status).toBe(401);

    const accepted = await worker.fetch(
      authorizedRequest("/admin/plan", "POST", body, "admin-test-token"),
      env,
    );
    expect(accepted.status).toBe(200);
    await expect(accepted.json()).resolves.toMatchObject({ ok: true, licenseId: "7", plan: "5000" });
    const status = await worker.fetch(authorizedRequest("/v1/status", "GET"), env);
    await expect(status.json()).resolves.toMatchObject({
      plan: "5000",
      limits: { smartModelMessages: 5, tutorMessages: 40 },
    });
    expect(kv.values.get("plan:7")).toContain('"plan":"5000"');

    const invalid = await worker.fetch(
      authorizedRequest(
        "/admin/plan",
        "POST",
        { licenseId: "18446744073709551616", plan: "5000" },
        "admin-test-token",
      ),
      env,
    );
    expect(invalid.status).toBe(400);
  });

  it("converts a photo data URL to Gemini inline image data", async () => {
    const upstream = vi.fn(async (_url: string | URL | Request, init?: RequestInit) => {
      expect(String(_url)).toContain("generativelanguage.googleapis.com");
      expect(new Headers(init?.headers).get("x-goog-api-key")).toBe("gemini-secret");
      const body = JSON.parse(String(init?.body)) as {
        contents: Array<{ parts: Array<Record<string, unknown>> }>;
      };
      expect(body.contents[0].parts[1]).toEqual({
        inline_data: { mime_type: "image/jpeg", data: "/9j/AA==" },
      });
      return Response.json({
        candidates: [{ content: { parts: [{ text: '{"text":"notes","uncertain":[],"blank":false}' }] } }],
      });
    });
    const { worker, env } = setup(upstream);
    const body = {
      task: "photo",
      messages: [
        { role: "system", content: "Transcribe." },
        {
          role: "user",
          content: [
            { type: "text", text: "Read this page." },
            { type: "image_url", image_url: { url: "data:image/jpeg;base64,/9j/AA==" } },
          ],
        },
      ],
      temperature: 0,
    };
    const response = await worker.fetch(authorizedRequest("/v1/ai", "POST", body), env);
    expect(response.status).toBe(200);
    await expect(response.json()).resolves.toMatchObject({
      choices: [{ message: { content: '{"text":"notes","uncertain":[],"blank":false}' } }],
    });
  });
});
