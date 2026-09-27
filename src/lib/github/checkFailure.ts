import type { CheckFailure } from "../api/github.ts";

/**
 * A failed check as one plain-text block to paste into an agent's terminal: which check failed
 * on which PR, then its output, annotations and log tail.
 */
export function failureReport(check: string, pullUrl: string, f: CheckFailure) {
  const parts = [`CI check "${check}" failed on ${pullUrl}`];
  const output = [f.title, f.summary].map((s) => s.trim()).filter(Boolean).join("\n\n");
  if (output) parts.push(output);
  if (f.annotations.length) {
    const lines = f.annotations.map((a) => `${a.path}${a.line ? `:${a.line}` : ""}: ${a.level}: ${a.title ? `${a.title}: ` : ""}${a.message}`);
    parts.push(["Annotations:", ...lines].join("\n"));
  }
  if (f.log) parts.push(`Job log, its last lines up to the error:\n${f.log}`);
  return parts.join("\n\n");
}
