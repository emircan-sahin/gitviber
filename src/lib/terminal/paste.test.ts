import assert from "node:assert/strict";
import { test } from "node:test";
import { pathPastes, shellPath } from "./paste.ts";

test("paths are escaped as Ghostty does", () => {
  assert.equal(shellPath("/Users/me/Ekran Resmi (1).png"), "/Users/me/Ekran\\ Resmi\\ \\(1\\).png");
  assert.equal(shellPath("/tmp/a'b\"c$d&e;f|g*h?i!j#k`l"), "/tmp/a\\'b\\\"c\\$d\\&e\\;f\\|g\\*h\\?i\\!j\\#k\\`l");
  assert.equal(shellPath("/tmp/[x]{y}<z>\\w\tv"), "/tmp/\\[x\\]\\{y\\}\\<z\\>\\\\w\\\tv");
  assert.equal(shellPath("/tmp/plain-name_1.txt"), "/tmp/plain-name_1.txt");
});

test("a name with a control character is left out", () => {
  assert.equal(shellPath("/tmp/evil\nrm -rf ~"), null);
  assert.equal(shellPath("/tmp/esc\x1b[201~"), null);
  assert.deepEqual(pathPastes(["/a b", "/tmp/x\ry", "/c"]), ["/a\\ b", " /c"]);
});

test("each path is its own paste, space-led after the first", () => {
  assert.deepEqual(pathPastes(["/a.png", "/b.png", "/c.png"]), ["/a.png", " /b.png", " /c.png"]);
  assert.deepEqual(pathPastes([]), []);
});
