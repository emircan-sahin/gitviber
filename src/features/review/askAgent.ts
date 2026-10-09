import { api } from "@/lib/api";
import { copyText } from "@/lib/app/clipboard";
import { toast } from "@/lib/app/toast";
import { handoffContext, type HandoffInput, handoffName, placedIn, riskPrompt } from "@/lib/review/handoff";
import { basename } from "@/lib/path";
import { getSettings } from "@/lib/settings";
import { agentFor, asPrompt, busyTitle, startAgentWith } from "@/lib/terminal/handoff";
import { pasteToAgent } from "@/lib/terminal/terminals";

/**
 * A guided review's section, risk or whole change handed to the user's agent CLI in the
 * worktree's terminal, for their own questions there: as a paste would put it, in a new Claude Code
 * session's prompt box (handoff.rs), or the agent already running there. Another CLI gets it on the
 * clipboard, as it has no way in at start.
 */

export type Ask = Omit<HandoffInput, "language">;

/** What's asked: a risk's look into it, else the user's question; null leaves it to them. */
function handoff(a: Ask, question = "") {
  const risk = !!a.about && "risk" in a.about;
  const q = question.trim();
  return { context: handoffContext({ ...a, language: getSettings().reviewLanguage }), prompt: risk ? riskPrompt(a.checkedOut) : q ? `My question: ${q}` : null };
}

/**
 * Where the agent works: the worktree that has the change checked out, so it reads the files on
 * disk and can fix them there, else `root` (the one open), reading the change with git.
 */
async function placed(root: string, a: Ask): Promise<{ root: string; a: Ask }> {
  if (a.checkedOut) return { root, a };
  const there = placedIn(a, await api.worktrees().catch(() => []));
  return { root: there.path ?? root, a: there.a };
}

/**
 * In a new terminal tab of the change's worktree: Claude Code with the review in its prompt box,
 * sent when there's a question (the user's, or a risk's), or another CLI with it on the clipboard.
 */
export async function askAgent(open: string, ask: Ask, question = "", model?: string) {
  const { root, a } = await placed(open, ask);
  if (root !== open) toast("info", `In worktree ${basename(root)}`, "Where the change is checked out, so the agent reads its files and can edit them.");
  const { context, prompt } = handoff(a, question);
  await startAgentWith(root, asPrompt(context, prompt), {
    name: handoffName(a.sel, a.about),
    send: !!prompt,
    copied: "Review copied for the agent",
    failed: "Couldn't hand the review to Claude Code",
    model,
  });
}

/**
 * Into the agent already running in the change's worktree, Enter left to the user: its session is warm.
 * Not while it works or asks something: the paste would land in its dialog or the user's draft.
 */
export async function pasteToRunningAgent(open: string, ask: Ask) {
  const { root, a } = await placed(open, ask);
  const agent = await agentFor(root);
  if (agent.to === "start") return toast("info", `No agent runs in ${basename(root)}'s terminal`, "Ask Agent starts one in a new terminal tab.");
  if (agent.to === "wait") return toast("info", busyTitle(agent), "Paste once it's done, or ask in a new session with Ask Agent.");
  const { context, prompt } = handoff(a);
  pasteToAgent(root, asPrompt(context, prompt));
}

export function copyAsPrompt(a: Ask) {
  const { context, prompt } = handoff(a);
  void copyText(asPrompt(context, prompt), "Review copied for the agent", "Paste it into the agent's terminal.");
}
