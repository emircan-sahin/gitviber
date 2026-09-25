import assert from "node:assert/strict";
import { test } from "node:test";
import { promptForm } from "./askpass.ts";

const form = (text: string, kind: string | null = null) => promptForm({ id: 1, text, kind, label: "git push", op: null });

test("git's HTTPS login asks a username, then a masked password", () => {
  assert.deepEqual(form("Username for 'https://github.com': "), { type: "text", title: "Sign in to github.com", label: "Username" });
  assert.deepEqual(form("Password for 'https://me@git.example.com:8443': "), { type: "secret", title: "Sign in to git.example.com", label: "Password or token" });
});

test("ssh's passphrase and password prompts are masked", () => {
  assert.deepEqual(form("Enter passphrase for key '/Users/me/.ssh/id_ed25519': "), {
    type: "secret",
    title: "Unlock your SSH key",
    label: "Passphrase for /Users/me/.ssh/id_ed25519",
  });
  assert.deepEqual(form("git@git.example.com's password: "), { type: "secret", title: "Sign in to git.example.com", label: "Password" });
  assert.equal(form("Verification code: ").type, "secret");
});

test("a new host's key is a yes/no with its fingerprint", () => {
  const text = `The authenticity of host 'github.com (140.82.121.4)' can't be established.
ED25519 key fingerprint is: SHA256:+DiY3wvvV6TuJJhbpZisF/zLDA0zPMSvHdkr4UvCOqU
This key is not known by any other names.
Are you sure you want to continue connecting (yes/no/[fingerprint])?`;
  const f = form(text);
  assert.equal(f.type, "confirm");
  if (f.type !== "confirm") return;
  assert.equal(f.title, "Connect to github.com?");
  assert.ok(f.hostKey);
  assert.match(f.details, /SHA256:\+DiY3/);
  assert.equal(f.question, "Are you sure you want to continue connecting (yes/no/[fingerprint])?");
  assert.equal(form("Continue connecting (yes/no)? ").type, "confirm");
  assert.equal(form("Allow use of key id_ed25519?", "confirm").type, "confirm");
});

test("ssh's notices only need dismissing", () => {
  assert.deepEqual(form("Confirm user presence for key ED25519-SK SHA256:x", "none"), { type: "notice", title: "ssh", details: "Confirm user presence for key ED25519-SK SHA256:x" });
});
