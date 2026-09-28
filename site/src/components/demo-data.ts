export interface DiffLine {
  kind: " " | "+" | "-";
  text: string;
}

/** A branch's pull request and the verdict of its checks. */
export interface Pull {
  number: number;
  ci: "success" | "failure" | "pending";
}

export interface Scene {
  agent: "claude" | "codex";
  branch: string;
  /** The linked worktree's folder. */
  folder: string;
  pull: Pull;
  /** Commits main doesn't have yet, and the branch's commits ahead of and behind its upstream. */
  commits: number;
  ahead: number;
  behind: number;
  /** The agent waits for the user: its worktree gets a dot. */
  calling?: boolean;
  /** When the branch's last commit was made. */
  time: string;
  prompt: string;
  /** What the agent printed, as it prints it. */
  output: string[];
  file: string;
  /** Line number of the first diff line, old side and new side. */
  start: [number, number];
  /** The other changed files in the worktree, with the open one first. */
  files: { path: string; add: number; del: number; status: "A" | "M"; viewed?: boolean }[];
  lines: DiffLine[];
}

const added = (text: string): DiffLine[] => text.split("\n").map((t) => ({ kind: "+", text: t }));

export const SCENES: Scene[] = [
  {
    agent: "claude",
    branch: "feat/fuzzy-search",
    folder: "acme-web-fuzzy",
    pull: { number: 128, ci: "success" },
    commits: 2,
    ahead: 0,
    behind: 1,
    time: "12m ago",
    prompt: "add fuzzy matching to quick open",
    output: ["⏺ Update(src/palette/QuickOpen.tsx)", "  ⎿  Updated src/palette/QuickOpen.tsx with 6 additions and 3 removals", "⏺ Write(src/search/fuzzy.ts)"],
    file: "src/search/fuzzy.ts",
    start: [0, 1],
    files: [
      { path: "src/search/fuzzy.ts", add: 16, del: 0, status: "A" },
      { path: "src/search/fuzzy.test.ts", add: 24, del: 0, status: "A" },
      { path: "src/palette/QuickOpen.tsx", add: 6, del: 3, status: "M", viewed: true },
    ],
    lines: added(`export function score(query: string, path: string): number {
  let q = 0;
  let run = 0;
  let total = 0;
  for (let i = 0; i < path.length && q < query.length; i++) {
    if (path[i].toLowerCase() !== query[q].toLowerCase()) {
      run = 0;
      continue;
    }
    // Consecutive hits and word starts count double.
    run++;
    total += run + (i === 0 || path[i - 1] === "/" ? 2 : 0);
    q++;
  }
  return q === query.length ? total : -1;
}`),
  },
  {
    agent: "codex",
    branch: "fix/auth-redirect",
    folder: "acme-web-auth",
    pull: { number: 131, ci: "failure" },
    commits: 1,
    ahead: 1,
    behind: 0,
    time: "1h ago",
    prompt: "the login page loops when the session expires",
    output: ["• Explored src/auth/session.ts, src/auth/login.ts", "• Edited src/auth/session.test.ts (+11 -0)", "• Edited src/auth/session.ts"],
    file: "src/auth/session.ts",
    start: [12, 12],
    files: [
      { path: "src/auth/session.ts", add: 4, del: 1, status: "M" },
      { path: "src/auth/session.test.ts", add: 11, del: 0, status: "M", viewed: true },
    ],
    lines: [
      { kind: " ", text: "export async function requireSession(req: Request) {" },
      { kind: " ", text: "  const session = await readSession(req);" },
      { kind: "-", text: '  if (!session) return redirect("/login");' },
      ...added(`  if (!session || session.expiresAt < Date.now()) {
    const next = encodeURIComponent(new URL(req.url).pathname);
    return redirect(\`/login?next=\${next}\`);
  }`),
      { kind: " ", text: "  return session;" },
      { kind: " ", text: "}" },
    ],
  },
  {
    agent: "claude",
    branch: "refactor/api-client",
    folder: "acme-web-api",
    pull: { number: 135, ci: "pending" },
    commits: 3,
    ahead: 0,
    behind: 0,
    calling: true,
    time: "34m ago",
    prompt: "move the fetch calls behind one api client",
    output: ["⏺ Update(src/features/users.ts)", "  ⎿  Updated src/features/users.ts with 2 additions and 9 removals", "⏺ Write(src/api/client.ts)"],
    file: "src/api/client.ts",
    start: [0, 1],
    files: [
      { path: "src/api/client.ts", add: 16, del: 0, status: "A" },
      { path: "src/features/orders.ts", add: 3, del: 14, status: "M" },
      { path: "src/features/users.ts", add: 2, del: 9, status: "M", viewed: true },
    ],
    lines: added(`const BASE = import.meta.env.VITE_API_URL;

export class ApiError extends Error {
  constructor(readonly status: number, message: string) {
    super(message);
  }
}

export async function api<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(\`\${BASE}\${path}\`, {
    ...init,
    headers: { "Content-Type": "application/json", ...init?.headers },
  });
  if (!res.ok) throw new ApiError(res.status, await res.text());
  return res.json() as Promise<T>;
}`),
  },
];

// Dark+ tells flow keywords (purple) from the ones that declare something (blue).
const FLOW = new Set("export import return if for continue await throw as".split(" "));
const DECLARE = new Set("function const let async new class extends constructor readonly super".split(" "));
const PRIMITIVES = new Set("string number boolean unknown void".split(" "));
const TOKEN = /(\/\/.*$)|("[^"]*"|`[^`]*`)|(\b\d+\b)|([A-Za-z_$][\w$]*)|(\s+)|([^\w\s"`$]+|\$)/g;

export type Token = { text: string; tone?: "kw" | "flow" | "str" | "num" | "fn" | "type" | "com" | "var" };

/** A tiny TypeScript highlighter, enough for the demo's lines and deterministic for prerendering. */
export function tokenize(line: string): Token[] {
  const out: Token[] = [];
  for (const m of line.matchAll(TOKEN)) {
    const [text, comment, str, num, word] = m;
    const rest = line.slice(m.index + text.length);
    if (comment) out.push({ text, tone: "com" });
    else if (str) out.push({ text, tone: "str" });
    else if (num) out.push({ text, tone: "num" });
    else if (word && FLOW.has(word)) out.push({ text, tone: "flow" });
    else if (word && DECLARE.has(word)) out.push({ text, tone: "kw" });
    else if (word && (PRIMITIVES.has(word) || /^[A-Z]/.test(word))) out.push({ text, tone: "type" });
    else if (word && /^\s*(<[^>]*>)?\(/.test(rest)) out.push({ text, tone: "fn" });
    else if (word) out.push({ text, tone: "var" });
    else out.push({ text });
  }
  return out;
}
