import type { KeyboardEvent } from "react";

/** ↑/↓ move between a list's rows, which take Enter themselves. */
export function arrows(e: KeyboardEvent<HTMLElement>) {
  if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
  const rows = [...e.currentTarget.querySelectorAll<HTMLElement>("[data-row]")];
  const to = rows[rows.indexOf(e.target as HTMLElement) + (e.key === "ArrowDown" ? 1 : -1)];
  if (!to) return;
  to.focus();
  to.scrollIntoView({ block: "nearest" });
  e.preventDefault();
}

