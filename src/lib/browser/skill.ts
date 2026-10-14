import type { SkillTarget } from "../api/browser.ts";
import { IS_LINUX } from "../platform.ts";

/** The skill is written while agents may use the browser and the setting's on; Linux tabs can't serve an agent yet. */
export const skillWanted = (s: { browserAgentControl: boolean; browserAgentSkill: boolean }, linux = IS_LINUX) =>
  s.browserAgentControl && s.browserAgentSkill && !linux;

/** Settings → Browser's line for one agent's skill: where it is, or why it isn't. */
export function skillLine({ agent, path, status, error }: SkillTarget, on: boolean): string {
  switch (status) {
    case "written":
      return `${agent}: ${path}`;
    case "absent":
      return `${agent}: not written (${path})`;
    case "newer":
      return `${agent}: ${path}, kept as a newer GitViber wrote it`;
    case "noAgent":
      return `${agent} not found`;
    case "theirs":
      return `A gitviber-browser skill of your own is at ${path}; GitViber leaves it`;
    case "failed":
      return `Couldn't ${on ? "write" : "remove"} ${path}${error ? `: ${error}` : ""}`;
  }
}

/** The lines Settings shows: with the skill off, only a failure to remove it, so no path reads as written. */
export const skillLines = (targets: SkillTarget[], on: boolean) => targets.filter((t) => on || t.status === "failed").map((t) => skillLine(t, on));
