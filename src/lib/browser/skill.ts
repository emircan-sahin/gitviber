import type { SkillTarget } from "../api/browser.ts";
import { IS_LINUX } from "../platform.ts";

/** The skill is written while agents may use the browser and the setting's on; Linux tabs can't serve an agent yet. */
export const skillWanted = (s: { browserAgentControl: boolean; browserAgentSkill: boolean }, linux = IS_LINUX) =>
  s.browserAgentControl && s.browserAgentSkill && !linux;

/** Settings → Browser's line for one agent's skill: where it goes, or why it doesn't. */
export function skillLine({ agent, path, status, error }: SkillTarget, on: boolean): string {
  switch (status) {
    case "noAgent":
      return `${agent} not found`;
    case "theirs":
      return `A gitviber-browser skill of your own is at ${path}; GitViber leaves it`;
    case "failed":
      return `Couldn't ${on ? "write" : "remove"} ${path}${error ? `: ${error}` : ""}`;
    default:
      return `${agent}: ${path}`;
  }
}
