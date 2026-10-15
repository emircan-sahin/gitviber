"""Tests of browser-check.py's parsing and table: python3 -I browser-check-test.py"""
import importlib.util
import json
import pathlib
import struct
import sys
import unittest
import zlib

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("check", pathlib.Path(__file__).with_name("browser-check.py"))
check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check)


def png(w, h):
    ihdr = struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)
    chunk = struct.pack(">I", len(ihdr)) + b"IHDR" + ihdr
    return b"\x89PNG\r\n\x1a\n" + chunk + struct.pack(">I", zlib.crc32(b"IHDR" + ihdr))


class Parsing(unittest.TestCase):
    def test_bridge_replies(self):
        self.assertEqual(check.bridge_value('{"ok":true,"value":"x y"}'), (True, "x y"))
        self.assertEqual(check.bridge_value('{"ok":true,"value":3}'), (True, "3"))
        self.assertEqual(check.bridge_value('{"ok":true,"value":null}'), (True, "null"))
        self.assertEqual(check.bridge_value('{"ok":false,"error":"boom"}'), (False, "boom"))
        self.assertFalse(check.bridge_value("")[0])

    def test_eval_output_with_or_without_page_markers(self):
        self.assertEqual(check.eval_value('"visible"\n'), "visible")
        self.assertEqual(check.eval_value("null"), None)
        self.assertEqual(check.eval_value("[12, 345.5]"), [12, 345.5])
        marked = "--- page content (untrusted) origin=http://a nonce=1 ---\n\"tok\"\n--- end page content nonce=1 ---"
        self.assertEqual(check.eval_value(marked), "tok")
        self.assertEqual(check.eval_value("undefined"), "undefined")

    def test_tab_state(self):
        listing = json.dumps({"ok": True, "data": {"tabs": [{"name": "a", "state": "sleeping"}, {"name": "b", "state": "awake"}]}})
        self.assertEqual(check.tab_state(listing, "a"), "sleeping")
        self.assertEqual(check.tab_state(listing, "c"), "missing")
        self.assertEqual(check.tab_state("GitViber isn't running.", "a"), "missing")

    def test_network_ids(self):
        listing = json.dumps({"ok": True, "data": {"dropped": 0, "entries": [
            {"id": 1, "kind": "resource", "url": "http://a/", "status": 200},
            {"id": 2, "kind": "fetch", "url": "http://a/api/hello?x=1", "status": 200},
            {"id": 3, "kind": "xhr", "url": "http://a/api/missing", "status": 404},
        ]}})
        self.assertEqual(check.network_id(listing, "fetch", "/api/hello", 200), 2)
        self.assertEqual(check.network_id(listing, "xhr", "/api/missing", 404), 3)
        self.assertIsNone(check.network_id(listing, "xhr", "/api/missing", 200))
        self.assertIsNone(check.network_id("not json", "fetch", "/", 200))

    def test_picture_path_from_old_and_new_replies(self):
        self.assertEqual(check.picture_path("/Users/x/Library/Caches/app/browser-picks/pick-1.png\n"),
                         "/Users/x/Library/Caches/app/browser-picks/pick-1.png")
        self.assertEqual(check.picture_path("Saved /tmp/a b/shot.jpg (800x600, 41 KB)"), "/tmp/a b/shot.jpg")
        self.assertIsNone(check.picture_path("Couldn't save the picture."))

    def test_snapshot_ref(self):
        out = '- main\n  - button "Log in" [ref=e3]\n  - link "Sign in" [ref=e12]\n  - heading "anon"'
        self.assertEqual(check.snapshot_ref(out, '"Sign in"'), "e12")
        self.assertEqual(check.snapshot_ref(out, "Log in"), "e3")
        self.assertIsNone(check.snapshot_ref(out, "anon"))

    def test_popup_name(self):
        out = "Clicked button \"Open popup\"\n→ popup opened as tab t1-popup1 (http://127.0.0.1:5/popup)"
        self.assertEqual(check.popup_name(out), "t1-popup1")
        self.assertIsNone(check.popup_name("→ no visible change"))

    def test_file_path(self):
        out = "→ tab t2: downloaded gitviber-check.txt (24 B): /Users/x/Downloads/GitViber/gitviber-check.txt"
        self.assertEqual(check.file_path(out, "gitviber-check.txt"), "/Users/x/Downloads/GitViber/gitviber-check.txt")
        self.assertIsNone(check.file_path("/a/gitviber-check.txt.part", "gitviber-check.txt"))
        self.assertIsNone(check.file_path("downloaded gitviber-check.txt", "gitviber-check.txt"))

    def test_shot_expect(self):
        self.assertEqual(check.shot_expect(1280, 800), (1280, 800))
        self.assertEqual(check.shot_expect(1280, 800, 0.5), (640, 400))
        self.assertEqual(check.shot_expect(1280, 800, 2), (2000, 1250))
        self.assertEqual(check.shot_expect(3000, 1000), (2000, 667))

    def test_png_size(self):
        self.assertEqual(check.png_size(png(2560, 1600)), (2560, 1600))
        self.assertIsNone(check.png_size(b"GIF89a" + b"\0" * 20))

    def test_fps(self):
        self.assertAlmostEqual(check.fps([100, 1000.0], [1300, 21000.0]), 60.0)
        self.assertEqual(check.fps([1, 5.0], [9, 5.0]), 0.0)

    def test_missing_needs(self):
        help_text = "  tab new [name]\n  session save <name>\n  network <id>\n"
        self.assertEqual(check.missing(help_text, ["session save", "network <id>"]), [])
        self.assertEqual(check.missing(help_text, ["find", "network <id>", "get"]), ["find", "get"])

    def test_memory(self):
        before = check.webcontent("  10 1000 /System/Library/Frameworks/WebKit.framework/XPCServices/com.apple.WebKit.WebContent.xpc/WebContent\n 11 50 /bin/zsh\n")
        after = check.webcontent("10 1000 com.apple.WebKit.WebContent\n12 103400 com.apple.WebKit.WebContent\n13 103000 com.apple.WebKit.WebContent\n")
        self.assertEqual(before, {10: 1000})
        self.assertEqual(check.memory_note(before, after, 2), "2 awake tabs: 2 new WebContent processes, +202 MB RSS in all (101 MB a tab)")
        self.assertIn("0 MB a tab", check.memory_note(before, before, 0))


class Table(unittest.TestCase):
    def test_aligned_with_a_summary_and_failing_on_a_fail(self):
        text, ok = check.table([["visibility", "PASS", "visible"], ["fps (20 s, no commands)", "FAIL", "31.0 rAF a second"], ["find", "SKIP"]])
        lines = text.splitlines()
        self.assertEqual(lines[0].split(), ["Check", "Result", "Note"])
        self.assertEqual(lines[2], "visibility               PASS    visible")
        self.assertEqual(lines[4], "find                     SKIP")
        self.assertEqual(lines[-1], "1 pass, 1 fail, 1 skip")
        self.assertFalse(ok)

    def test_passes_only_with_a_pass_and_no_fail(self):
        self.assertTrue(check.table([["a", "PASS"], ["memory", "INFO", "3 tabs"], ["b", "SKIP"]])[1])
        self.assertFalse(check.table([["b", "SKIP"]])[1])
        self.assertEqual(check.table([])[0].splitlines()[-1], "no checks ran")


if __name__ == "__main__":
    unittest.main()
