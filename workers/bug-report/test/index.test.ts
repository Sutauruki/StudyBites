import { describe, expect, it, vi } from "vitest";
import {
  createWorker,
  escapeHtml,
  validateReport,
  type Env,
  type RateKv,
} from "../src/index";

class MemoryKv implements RateKv {
  readonly values = new Map<string, string>();

  async get(key: string): Promise<string | null> {
    return this.values.get(key) ?? null;
  }

  async put(key: string, value: string): Promise<void> {
    this.values.set(key, value);
  }
}

const PNG = "iVBORw0KGgo=";
const report = (patch: Record<string, unknown> = {}) => ({
  id: "00000000-0000-4000-8000-000000000001",
  deviceId: "00000000-0000-4000-8000-000000000002",
  title: "Something happened",
  details: "The button did not respond.",
  category: "bug",
  diag: { platform: "Android", appVersion: "1.2.3" },
  attachments: [{ filename: "screen.png", mime: "image/png", data: PNG }],
  ...patch,
});

function setup(fetcher: typeof fetch = vi.fn(async () => new Response(null, { status: 200 }))) {
  const kv = new MemoryKv();
  const env: Env = {
    RATE: kv,
    RESEND_API_KEY: "test-key",
    BUG_REPORT_TO: "owner@example.test",
  };
  return { worker: createWorker(fetcher), env, kv, fetcher };
}

function request(body: unknown, headers: HeadersInit = {}): Request {
  const requestHeaders = new Headers({ "Content-Type": "application/json" });
  new Headers(headers).forEach((value, key) => requestHeaders.set(key, value));
  return new Request("https://report.test/", {
    method: "POST",
    headers: requestHeaders,
    body: JSON.stringify(body),
  });
}

describe("bug report worker", () => {
  it("strips controls and escapes every HTML metacharacter", () => {
    expect(escapeHtml(`<script>\u0000 & " ' \n`)).toBe(
      "&lt;script&gt; &amp; &quot; &#39; <br>",
    );
  });

  it("rejects invalid fields and mismatched screenshot magic bytes", async () => {
    const { worker, env } = setup();
    const response = await worker.fetch(
      request(report({ attachments: [{ filename: "screen.png", mime: "image/png", data: "AA==" }] })),
      env,
    );
    expect(response.status).toBe(400);
    await expect(response.json()).resolves.toMatchObject({ ok: false });
  });

  it("returns a clear validation response for oversized reports", async () => {
    const { worker, env } = setup();
    const response = await worker.fetch(
      request(report({ details: "x".repeat(2001) })),
      env,
    );
    expect(response.status).toBe(400);
    await expect(response.json()).resolves.toMatchObject({
      error: "Describe what happened in 1 to 2,000 characters.",
    });
  });

  it("redacts technical details and rejects unapproved diagnostic fields", () => {
    const validated = validateReport(
      report({
        diag: {
          webview: "Bearer abcdef123 /home/student/notes.pdf",
          appVersion: "AIza123456789012345678901234567890123456",
        },
      }),
    );
    expect("report" in validated).toBe(true);
    if ("report" in validated) {
      expect(validated.report.diag.webview).not.toContain("abcdef123");
      expect(validated.report.diag.webview).not.toContain("/home/student");
      expect(validated.report.diag.appVersion).not.toContain("AIza");
    }

    expect(
      validateReport(report({ diag: { notes: "private student material" } })),
    ).toHaveProperty("error");
  });

  it("deduplicates a successfully delivered report by id", async () => {
    const send = vi.fn(async () => new Response(null, { status: 200 }));
    const { worker, env } = setup(send);
    const requestBody = report();
    const first = await worker.fetch(request(requestBody), env);
    const retry = await worker.fetch(request(requestBody), env);
    expect(first.status).toBe(200);
    expect(retry.status).toBe(200);
    expect(send).toHaveBeenCalledTimes(1);
  });

  it("enforces the per-device daily limit", async () => {
    const { worker, env } = setup();
    for (let index = 0; index < 5; index += 1) {
      const response = await worker.fetch(
        request(report({ id: `00000000-0000-4000-8000-${String(index + 10).padStart(12, "0")}` })),
        env,
      );
      expect(response.status).toBe(200);
    }
    const limited = await worker.fetch(
      request(report({ id: "00000000-0000-4000-8000-000000000099" })),
      env,
    );
    expect(limited.status).toBe(429);
    await expect(limited.json()).resolves.toMatchObject({ ok: false, resetsAt: expect.any(String) });
  });

  it("enforces the per-IP daily limit across different devices", async () => {
    const { worker, env } = setup();
    for (let index = 0; index < 10; index += 1) {
      const suffix = String(index + 100).padStart(12, "0");
      const response = await worker.fetch(
        request(
          report({
            id: `00000000-0000-4000-8000-${suffix}`,
            deviceId: `00000000-0000-4000-8000-${String(index + 200).padStart(12, "0")}`,
          }),
          { "CF-Connecting-IP": "192.0.2.1" },
        ),
        env,
      );
      expect(response.status).toBe(200);
    }
    const limited = await worker.fetch(
      request(
        report({
          id: "00000000-0000-4000-8000-000000000999",
          deviceId: "00000000-0000-4000-8000-000000000998",
        }),
        { "CF-Connecting-IP": "192.0.2.1" },
      ),
      env,
    );
    expect(limited.status).toBe(429);
  });

  it("enforces the global daily limit across devices and source IPs", async () => {
    const { worker, env } = setup();
    for (let index = 0; index < 60; index += 1) {
      const suffix = String(index + 100).padStart(12, "0");
      const response = await worker.fetch(
        request(
          report({
            id: `00000000-0000-4000-8000-${suffix}`,
            deviceId: `00000000-0000-4000-8000-${String(index + 200).padStart(12, "0")}`,
          }),
          { "CF-Connecting-IP": `192.0.2.${index + 1}` },
        ),
        env,
      );
      expect(response.status).toBe(200);
    }
    const limited = await worker.fetch(
      request(
        report({
          id: "00000000-0000-4000-8000-000000000999",
          deviceId: "00000000-0000-4000-8000-000000000998",
        }),
        { "CF-Connecting-IP": "198.51.100.1" },
      ),
      env,
    );
    expect(limited.status).toBe(429);
  });

  it("sends plain text, escaped HTML, an idempotency key and base64 attachments", async () => {
    const send = vi.fn(async (_url: string | URL | Request, init?: RequestInit) => {
      const payload = JSON.parse(String(init?.body)) as {
        html: string;
        text: string;
        attachments: Array<{ filename: string; content: string }>;
      };
      expect(payload.html).toContain("&lt;script&gt;");
      expect(payload.text).toContain("<script>");
      expect(payload.attachments[0]).toEqual({ filename: "screen.png", content: PNG });
      expect(new Headers(init?.headers).get("Idempotency-Key")).toBe(
        "00000000-0000-4000-8000-000000000001",
      );
      return new Response(null, { status: 200 });
    });
    const { worker, env } = setup(send);
    const response = await worker.fetch(
      request(report({ title: "<script>", details: "A detail." })),
      env,
    );
    expect(response.status).toBe(200);
  });
});
