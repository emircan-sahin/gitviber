import assert from "node:assert/strict";
import { test } from "node:test";
import { skillLine, skillWanted } from "./skill.ts";

const claude = { agent: "Claude Code", path: "~/.claude/skills/gitviber-browser/SKILL.md" };

test("the skill is wanted only with agent control and the setting both on, and not on Linux", () => {
  assert.equal(skillWanted({ browserAgentControl: true, browserAgentSkill: true }, false), true);
  assert.equal(skillWanted({ browserAgentControl: false, browserAgentSkill: true }, false), false);
  assert.equal(skillWanted({ browserAgentControl: true, browserAgentSkill: false }, false), false);
  assert.equal(skillWanted({ browserAgentControl: true, browserAgentSkill: true }, true), false);
});

test("each agent's line names the path it writes, or why it doesn't", () => {
  assert.equal(skillLine({ ...claude, status: "written" }, true), "Claude Code: ~/.claude/skills/gitviber-browser/SKILL.md");
  assert.equal(skillLine({ ...claude, status: "none" }, false), "Claude Code: ~/.claude/skills/gitviber-browser/SKILL.md");
  assert.equal(skillLine({ ...claude, status: "noAgent" }, true), "Claude Code not found");
  assert.equal(
    skillLine({ ...claude, status: "theirs" }, true),
    "A gitviber-browser skill of your own is at ~/.claude/skills/gitviber-browser/SKILL.md; GitViber leaves it",
  );
  assert.equal(
    skillLine({ ...claude, status: "failed", error: "Permission denied (os error 13)" }, true),
    "Couldn't write ~/.claude/skills/gitviber-browser/SKILL.md: Permission denied (os error 13)",
  );
  assert.equal(skillLine({ ...claude, status: "failed" }, false), "Couldn't remove ~/.claude/skills/gitviber-browser/SKILL.md");
});
