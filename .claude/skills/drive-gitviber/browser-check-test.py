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


def png_rows(colors):
    """A 2 px wide RGBA PNG, a row to a colour, its rows filtered each of the five ways."""
    def chunk(tag, data):
        return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data))
    raw, prev = b"", bytes(8)
    for i, (r, g, b) in enumerate(colors):
        line = bytes([r, g, b, 255] * 2)
        left = lambda j: line[j - 4] if j >= 4 else 0
        up = lambda j: prev[j]
        corner = lambda j: prev[j - 4] if j >= 4 else 0
        def paeth(j):
            a, b2, c = left(j), up(j), corner(j)
            p = a + b2 - c
            pa, pb, pc = abs(p - a), abs(p - b2), abs(p - c)
            return a if pa <= pb and pa <= pc else b2 if pb <= pc else c
        kind = i % 5
        guess = [lambda j: 0, left, up, lambda j: (left(j) + up(j)) // 2, paeth][kind]
        raw += bytes([kind]) + bytes((line[j] - guess(j)) & 255 for j in range(8))
        prev = line
    ihdr = struct.pack(">IIBBBBB", 2, len(colors), 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")


class Parsing(unittest.TestCase):
    def test_a_full_page_screenshot_is_the_tall_page_once(self):
        rows = [check.tall_expect(i) for i in range(3000)]
        self.assertEqual(check.png_column(png_rows(rows), 1), rows)
        self.assertIsNone(check.tall_check(rows))
        # A viewport wider than 2000 px takes the page at less than 1 px a CSS px.
        self.assertEqual(check.tall_scale(1280, 800), 1.0)
        self.assertAlmostEqual(check.tall_scale(3440, 1128), 2000 / 3440)
        half = [check.tall_expect(i / 0.5) for i in range(1500)]
        self.assertIsNone(check.tall_check(half, 0.5))
        self.assertIn("rows", check.tall_check(half))
        # The header drawn again below the first tile is a fault, and so is a missing row.
        again = rows[:800] + [(255, 0, 0)] * 60 + rows[860:]
        self.assertIn("row 803", check.tall_check(again))
        self.assertIn("2980 rows", check.tall_check(rows[:-20]))

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
        self.assertIsNone(check.popup_name("→ no DOM change"))

    def test_popup_replies_as_the_code_writes_them(self):
        # popups.rs and notes.rs on feat/integration-popups.
        click = 'Clicked button "Open popup" → no DOM change\n→ popup opened as tab t1-popup1 (http://127.0.0.1:5/popup)'
        self.assertEqual(check.popup_name(click), "t1-popup1")
        closed = "→ tab t1-popup1 closed: its page called window.close()"
        self.assertTrue(check.closed_note(closed, "t1-popup1"))
        self.assertTrue(check.closed_note('"ok"\n' + closed + "\n", "t1-popup1"))
        self.assertFalse(check.closed_note(closed, "t1-popup2"))

    def test_download_reply_as_the_code_writes_it(self):
        # downloads.rs `wait --download`; a click's note says it in lower case, without the type.
        out = "Downloaded gitviber-check.txt (24 B, text/plain): /Users/x/Library/Caches/app.gitviber.desktop/browser-downloads/ab/gitviber-check.txt"
        self.assertEqual(check.download(out), ("gitviber-check.txt", "24 B", "text/plain",
                         "/Users/x/Library/Caches/app.gitviber.desktop/browser-downloads/ab/gitviber-check.txt"))
        self.assertEqual(check.download("Downloaded report (2).pdf (24 KB, application/pdf): /c/report (2).pdf"),
                         ("report (2).pdf", "24 KB", "application/pdf", "/c/report (2).pdf"))
        self.assertIsNone(check.download("→ tab t2: downloaded report.pdf (24 KB): /c/report.pdf"))
        self.assertIsNone(check.download("Timed out after 10000 ms waiting for a download"))

    def test_eval_value_without_notes(self):
        self.assertEqual(check.eval_value('"hi-from-popup"\n→ popup opened as tab t1-popup1 (http://a/popup)'), "hi-from-popup")

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
