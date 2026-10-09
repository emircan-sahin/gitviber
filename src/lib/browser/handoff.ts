import { forTerminal } from "../review/notes";
import { asPrompt, startAgentWith } from "../terminal/handoff";

/**
 * Something from a browser tab (a picked element, the page's errors) for `root`'s agent, as Ask Agent
 * hands a review: a new session in a terminal tab there with it in the prompt box, sent with the
 * user's question, left there for them without one.
 */
export function askAgentAbout(root: string, content: string, name: string, question = "", model?: string) {
  const q = question.trim();
  void startAgentWith(root, asPrompt(forTerminal(content), q ? `My question: ${q}` : null), {
    name,
    send: !!q,
    copied: "Copied for the agent",
    failed: "Couldn't hand it to Claude Code",
    model,
  });
}
