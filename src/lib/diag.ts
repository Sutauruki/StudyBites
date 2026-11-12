const recentErrors: string[] = [];
let handlersInstalled = false;

function redact(value: string): string {
  return value
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
    .replace(/\b[A-Za-z0-9+/_=-]{80,}\b/g, "[redacted]")
    .replace(/[\u0000-\u001f\u007f]/g, " ")
    .trim();
}

export function recordDiagnosticError(value: string): void {
  const message = summarizeError(value);
  if (!message || recentErrors[recentErrors.length - 1] === message) return;
  recentErrors.push(message);
  if (recentErrors.length > 20) recentErrors.shift();
}

function summarizeError(value: string): string {
  const message = redact(value).toLowerCase();
  if (message.includes("timeout") || message.includes("timed out")) return "Request timed out";
  if (message.includes("network") || message.includes("fetch") || message.includes("connection")) {
    return "Network request failed";
  }
  if (message.includes("permission") || message.includes("denied")) return "Permission denied";
  if (message.includes("database") || message.includes("storage")) return "Storage operation failed";
  return "Application error";
}

export function installDiagnosticHandlers(): void {
  if (handlersInstalled || typeof window === "undefined") return;
  handlersInstalled = true;
  window.addEventListener("error", (event) => {
    recordDiagnosticError(event.message || "Unhandled window error");
  });
  window.addEventListener("unhandledrejection", (event) => {
    const reason = event.reason;
    recordDiagnosticError(
      reason instanceof Error ? reason.message : String(reason ?? "Unhandled rejection"),
    );
  });
}

export function collectDiagnostics(context: {
  appVersion: string;
  view: string;
  licenseState: string;
}): Record<string, string> {
  const userAgent = typeof navigator === "undefined" ? "" : navigator.userAgent;
  const platform = /Android/i.test(userAgent)
    ? "Android"
    : /Windows/i.test(userAgent)
      ? "Windows"
      : "browser";
  const errors = recentErrors.slice(-12).map((error) => redact(error).slice(0, 160));
  const timeZone = Intl.DateTimeFormat().resolvedOptions().timeZone || "unknown";

  return {
    appVersion: redact(context.appVersion).slice(0, 40),
    platform,
    webview: redact(userAgent).slice(0, 180),
    screenSize:
      typeof screen === "undefined" ? "unknown" : `${screen.width}x${screen.height}`,
    language: typeof navigator === "undefined" ? "unknown" : navigator.language.slice(0, 40),
    timeZone: timeZone.slice(0, 80),
    online: typeof navigator === "undefined" ? "unknown" : String(navigator.onLine),
    view: redact(context.view).slice(0, 60),
    licenseState: redact(context.licenseState).slice(0, 40),
    recentErrors: JSON.stringify(errors),
  };
}
