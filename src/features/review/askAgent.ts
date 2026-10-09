import { api, errorMessage } from "@/lib/api";
import { copyText } from "@/lib/app/clipboard";
import { toast } from "@/lib/app/toast";
import { effortOf, modelOf, presetOf, programOf, reviewAgent, SUGGEST_PRESETS } from "@/lib/git/suggest";
import { handoffContext, type HandoffInput, handoffName, riskPrompt } from "@/lib/review/handoff";
import { getSettings } from "@/lib/settings";
import { refreshAgents } from "@/lib/terminal/agents";
import { openTerminal, pasteToAgent, worktreeAgent } from "@/lib/terminal/terminals";

/**
 * A guided review's section, risk or whole change handed to the user's agent CLI in the
 * worktree's terminal, for their own questions there: as a paste would put it, in a new Claude Code
 * session's prompt box (handoff.rs), or the agent already running there. Another CLI gets it on the
 * clipboard, as it has no way in at start.
 */

export type Ask = Omit<HandoffInput, "language">;

/** What's asked: a risk's look into it, else the user's question; null leaves it to them. */
function handoff(a: Ask, question = "") {
  const s = getSettings();
  const risk = !!a.about && "risk" in a.about;
  const q = question.trim();
  return { settings: s, context: handoffContext({ ...a, language: s.reviewLanguage }), prompt: risk ? riskPrompt(a.checkedOut) : q ? `My question: ${q}` : null };
}

/**
 * The context and what's asked, as one prompt to paste. Sent as it is (pasting, then Enter, is the
 * habit), it has the agent say it's ready rather than answer a question that isn't there.
 */
const asPrompt = (context: string, prompt: string | null) => `${context}\n\n${prompt ?? "My question follows. If none does, just say in one line that you're ready, and wait for it."}`;

/**
 * In a new terminal tab of `root`: Claude Code with the review in its prompt box, sent when there's
 * a question (the user's, or a risk's), or another CLI with it on the clipboard.
 */
export async function askAgent(root: string, a: Ask, question = "") {
  const { settings, context, prompt } = handoff(a, question);
  const { command, models, efforts } = reviewAgent(settings);
  const preset = presetOf(command);
  if (programOf(command).toLowerCase() !== "claude") {
    // A preset's own session; llm and a custom command have none known.
    const program = preset && "session" in SUGGEST_PRESETS[preset] ? programOf(command) : null;
    void copyText(asPrompt(context, prompt), "Review copied for the agent", program ? `Paste it into ${program}, starting in the terminal.` : "Paste it into your agent.");
    if (program) openTerminal(root, program);
    return;
  }
  try {
    // A preset's model and effort, as the review ran with; a custom command's are its own business.
    const line = await api.handoffCommand({
      command,
      model: (preset && modelOf(preset, models)) || null,
      effort: (preset && effortOf(preset, efforts)) || null,
      name: handoffName(a.sel, a.about),
      text: asPrompt(context, prompt),
      send: !!prompt,
    });
    openTerminal(root, line);
  } catch (e) {
    toast("error", "Couldn't hand the review to Claude Code", errorMessage(e));
  }
}

/**
 * Into the agent already running in `root`'s terminal, Enter left to the user: its session is warm.
 * Not while it works or asks something: the paste would land in its dialog or the user's draft.
 */
export async function pasteToRunningAgent(root: string, a: Ask) {
  // A session started a moment ago isn't known until it's looked up.
  await refreshAgents();
  const agent = worktreeAgent(root);
  if (!agent) return toast("info", "No agent runs in this worktree's terminal", "Ask Agent starts one in a new terminal tab.");
  if (agent.state === "working" || agent.state === "waiting")
    return toast("info", `${agent.name} is ${agent.state === "working" ? "working" : "waiting on you"}`, "Paste once it's done, or ask in a new session with Ask Agent.");
  const { context, prompt } = handoff(a);
  pasteToAgent(root, asPrompt(context, prompt));
}

export function copyAsPrompt(a: Ask) {
  const { context, prompt } = handoff(a);
  void copyText(asPrompt(context, prompt), "Review copied for the agent", "Paste it into the agent's terminal.");
}
