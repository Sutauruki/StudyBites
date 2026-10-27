import { mockBackend } from "./mock";
import { tauriBackend } from "./tauri";
import type { Backend } from "./types";

/**
 * Inside the Tauri window this is true and the real Rust backend is used; in a
 * plain browser (Codespaces, `npm run dev:web`) the mock takes over.
 */
export const isDesktop: boolean =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export const backend: Backend = isDesktop ? tauriBackend : mockBackend;

/**
 * Errors from Tauri commands arrive as plain strings; the mock throws strings
 * too. Anything else gets a readable fallback rather than "[object Object]".
 */
export function errorText(err: unknown): string {
  if (typeof err === "string") return err;
  if (err instanceof Error) return err.message;
  return "Something went wrong.";
}

export * from "./types";
