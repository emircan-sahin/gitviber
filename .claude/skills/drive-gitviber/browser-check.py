"""The parts of browser-check.sh that parse or print, so they can be tested apart from a running
app. Run with `python3 -I browser-check.py <command> ...`; replies to parse come on stdin."""
import json
import re
from collections import Counter
import socket
import struct
import sys

PAGE_MARK = re.compile(r"^--- (end )?page content\b")


def bridge_value(reply):
    """dev_bridge.rs's reply: (True, the value as text) or (False, the error)."""
    try:
        r = json.loads(reply)
    except ValueError:
        return False, f"no reply from the dev bridge: {reply[:200]!r}"
    if not r.get("ok"):
        return False, str(r.get("error"))
    v = r.get("value")
    return True, v if isinstance(v, str) else json.dumps(v)


def eval_value(out):
    """What `eval` printed, without the page-content markers a newer build puts around it or
    the notes (`→ …`) it ends with; JSON is decoded, so "visible" comes back as visible and
    [1, 2] as a list."""
    text = "\n".join(l for l in out.splitlines() if not PAGE_MARK.match(l) and not l.startswith("→ ")).strip()
    try:
        return json.loads(text)
    except ValueError:
        return text


def tab_state(listing, name):
    """A tab's state in `--json tab list`'s reply, or "missing"."""
    try:
        tabs = json.loads(listing)["data"]["tabs"]
    except (ValueError, KeyError, TypeError):
        return "missing"
    return next((t.get("state", "?") for t in tabs if t.get("name") == name), "missing")


def tab_audio(listing, name):
    """Whether a tab's `audio` in `--json tab list`'s reply is true: "true", "false" or "missing"."""
    try:
        tabs = json.loads(listing)["data"]["tabs"]
    except (ValueError, KeyError, TypeError):
        return "missing"
    return next((str(bool(t.get("audio"))).lower() for t in tabs if t.get("name") == name), "missing")


def network_id(listing, kind, part, status):
    """The id of the first request in `--json network`'s reply of that kind, with `part` in its
    URL and that status; None if there's none."""
    try:
        entries = json.loads(listing)["data"]["entries"]
    except (ValueError, KeyError, TypeError):
        return None
    for e in entries:
        if (str(e.get("kind", "")).lower() == kind and part in str(e.get("url", ""))
                and e.get("status") == status):
            return e.get("id")
    return None


def picture_path(out):
    """The picture's path in `screenshot`'s reply: the first absolute path to a .png or .jpg."""
    m = re.search(r"(/[^\n]*?\.(?:png|jpe?g))\b", out)
    return m.group(1) if m else None


def snapshot_ref(out, text):
    """The ref of the first snapshot line that names `text`: `button "Log in" [ref=e3]` gives e3."""
    for line in out.splitlines():
        m = re.search(r"\[ref=(e\d+)\]", line)
        if m and text in line:
            return m.group(1)
    return None


def popup_name(out):
    """The tab a reply says a popup opened as: `→ popup opened as tab t1-popup1 (…)`."""
    m = re.search(r"popup opened as tab (\S+)", out)
    return m.group(1) if m else None


def closed_note(out, name):
    """Whether a reply tells that tab `name` closed as its page called window.close():
    `→ tab t1-popup1 closed: its page called window.close()`."""
    return f"tab {name} closed: its page called window.close()" in out


def download(out):
    """`wait --download`'s reply as (name, size, type, path):
    `Downloaded report.pdf (24 KB, application/pdf): /c/report.pdf`."""
    m = re.search(r"^Downloaded (.+?) \(([^,()]+), ([^()]+)\): (/.*)$", out, re.M)
    return m.groups() if m else None


def file_path(out, name):
    """The first absolute path in a reply that ends with the file's name."""
    m = re.search(r"(/[^\n]*?/" + re.escape(name) + r")(?![\w.-])", out)
    return m.group(1) if m else None


def shot_expect(w, h, scale=1.0, cap=2000):
    """A screenshot's pixels for a CSS size: w×h × scale, its longest side `cap` at most."""
    w, h = w * scale, h * scale
    k = min(1.0, cap / max(w, h))
    return round(w * k), round(h * k)


SKILL_HEAD = "---\nname: gitviber-browser\n"
SKILL_MARKER = re.compile(r"^<!-- managed by GitViber (\d+\.\d+\.\d+\S*); edits are replaced -->$")


def skill_version(text):
    """The GitViber version that wrote a gitviber-browser SKILL.md, as browser/skill.rs marks it:
    its name first, and the marker the first line after the frontmatter. None for anyone else's."""
    if not text.startswith(SKILL_HEAD):
        return None
    rest = text[len(SKILL_HEAD):]
    body = rest[4:] if rest.startswith("---\n") else rest.partition("\n---\n")[2]
    m = SKILL_MARKER.match(body.split("\n", 1)[0])
    return m.group(1) if m else None


# A browser other than GitViber's: a tool or a command line naming one.
OTHER_BROWSER = re.compile(r"mcp__claude-in-chrome|mcp__playwright|playwright|puppeteer|chrome-devtools|Google Chrome|chromium", re.I)


def browser_commands(command):
    """The `gitviber browser` subcommands in a shell command line, flags and --tab's value skipped."""
    found = []
    for m in re.finditer(r"gitviber\s+browser\b([^;&|\n]*)", command):
        words = m.group(1).split()
        while words and words[0].startswith("--"):
            flag = words.pop(0)
            if flag == "--tab" and words:
                words.pop(0)
        found.append(words[0] if words else "help")
    return found


def agent_summary(stream):
    """A `claude -p --output-format stream-json --verbose` run, summed up without its transcript:
    the answer, the gitviber browser subcommands it ran, any other browser it reached for."""
    answer, error, tools, commands, other = None, None, 0, [], []
    for line in stream.splitlines():
        try:
            msg = json.loads(line)
        except ValueError:
            continue
        if msg.get("type") == "result":
            answer, error = str(msg.get("result", "")), bool(msg.get("is_error"))
        content = msg.get("message", {}).get("content") if msg.get("type") == "assistant" else None
        for block in content if isinstance(content, list) else []:
            if block.get("type") != "tool_use":
                continue
            tools += 1
            name, given = block.get("name", ""), block.get("input", {})
            command = given.get("command", "") if isinstance(given, dict) else ""
            commands += browser_commands(command)
            outside = " ".join(filter(None, [name, re.sub(r"gitviber\s+browser\b[^;&|\n]*", "", command)]))
            if OTHER_BROWSER.search(outside):
                other.append(name if name != "Bash" else "Bash: " + command[:40])
    return {"answer": answer, "error": error, "tools": tools, "commands": commands, "other": other}


def agent_verdict(summary, status):
    """PASS when the answer holds the status, the run used gitviber browser (when it shows its
    tools) and no other browser; a one-line note either way."""
    answer = summary["answer"]
    said = "no answer" if answer is None else f"answered {answer.strip()[:40]!r}"
    used = Counter(summary["commands"])
    note = f"{said}; {summary['tools']} tool calls, gitviber browser " + (
        ", ".join(f"{c}×{n}" if n > 1 else c for c, n in used.items()) or "never")
    if summary["other"]:
        note += "; other browser: " + ", ".join(sorted(set(summary["other"])))[:80]
    ok = (answer is not None and not summary["error"] and re.search(rf"\b{status}\b", answer) is not None
          and not summary["other"] and (summary["tools"] == 0 or bool(summary["commands"])))
    return ok, note


def form_submit(log, run):
    """Whether the test site saw run `run`'s form posted from a WebKit view that isn't Chrome's
    (an agent tab's WKWebView), and the user agent it came with."""
    for line in log.splitlines():
        if line.startswith(f"form-submit run={run} "):
            ua = line.partition(" ua=")[2]
            ok = "AppleWebKit" in ua and not re.search(r"Chrome|Chromium|Firefox|Edg/", ua)
            return ok, ua
    return False, None


def png_size(data):
    """A PNG's width and height from its IHDR."""
    if data[:8] != b"\x89PNG\r\n\x1a\n" or data[12:16] != b"IHDR":
        return None
    return struct.unpack(">II", data[16:24])


def png_column(data, x):
    """The RGB of each row of a PNG (8-bit, not interlaced) down column x."""
    import zlib
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError("not a PNG")
    w, h, depth, kind, _, _, interlace = struct.unpack(">IIBBBBB", data[16:29])
    if depth != 8 or interlace or kind not in (2, 6):
        raise ValueError("a PNG this reader doesn't take")
    bpp = 3 if kind == 2 else 4
    idat, i = b"", 8
    while i < len(data):
        n, tag = struct.unpack(">I4s", data[i:i + 8])
        if tag == b"IDAT":
            idat += data[i + 8:i + 8 + n]
        i += 12 + n
    raw, stride = zlib.decompress(idat), w * bpp
    prev, rows, at = bytearray(stride), [], 0
    for _ in range(h):
        f, line, at = raw[at], bytearray(raw[at + 1:at + 1 + stride]), at + 1 + stride
        for j in range(stride):
            a = line[j - bpp] if j >= bpp else 0
            b, c = prev[j], (prev[j - bpp] if j >= bpp else 0)
            if f == 1:
                line[j] = (line[j] + a) & 255
            elif f == 2:
                line[j] = (line[j] + b) & 255
            elif f == 3:
                line[j] = (line[j] + (a + b) // 2) & 255
            elif f == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                line[j] = (line[j] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
        rows.append(tuple(line[x * bpp:x * bpp + 3]))
        prev = line
    return rows


def tall_expect(y, total=3000):
    """The colour at `y` CSS px of site.py's /tall page: its fixed header, its fixed footer, or the
    band under them."""
    if y < 60:
        return (255, 0, 0)
    if y >= total - 40:
        return (0, 0, 0)
    i = int(y) // 200
    return (40 + i * 13, 100, 200 - i * 9)


def tall_scale(view_w, view_h, scale=1.0):
    """The pixels a CSS px of a screenshot gets: `scale`, less where its longest side would pass 2000."""
    return min(scale, 2000 / max(view_w, view_h))


def tall_check(rows, scale=1.0):
    """Whether the rows of a full-page screenshot of /tall (all its pictures, top to bottom, at
    `scale` px a CSS px) are its page, the header and the footer each once. Rows by an edge are
    left out. The first fault, or None."""
    if abs(len(rows) - 3000 * scale) > 5:
        return f"{len(rows)} rows, want {3000 * scale:.0f}"
    edges = [e * scale for e in (60, 2960, *range(0, 3001, 200))]
    for i, got in enumerate(rows):
        if any(abs(i - e) <= 2 for e in edges):
            continue
        want = tall_expect(i / scale)
        if any(abs(g - w) > 6 for g, w in zip(got, want)):
            return f"row {i} is {got}, want {want}"
    return None


def fps(before, after):
    """Frames a second between two `[frames, performance.now()]` readings."""
    (f1, t1), (f2, t2) = before, after
    return (f2 - f1) / ((t2 - t1) / 1000) if t2 > t1 else 0.0


def missing(help_text, needs):
    """The tokens of a check's needs that `gitviber browser help` doesn't mention."""
    return [n for n in needs if n not in help_text]


def webcontent(ps_out):
    """{pid: rss in KB} of WebKit's page processes in `ps -axo pid=,rss=,comm=`."""
    procs = {}
    for line in ps_out.splitlines():
        parts = line.split(None, 2)
        if len(parts) == 3 and "WebContent" in parts[2] and parts[0].isdigit():
            procs[int(parts[0])] = int(parts[1])
    return procs


def memory_note(before, after, tabs):
    new = {p: kb for p, kb in after.items() if p not in before}
    grown = sum(after.values()) - sum(before.values())
    each = grown / 1024 / tabs if tabs else 0
    return (f"{tabs} awake tabs: {len(new)} new WebContent processes, "
            f"{grown / 1024:+.0f} MB RSS in all ({each:.0f} MB a tab)")


def table(rows):
    """The results as a table, and whether all passed: rows of (check, result, note)."""
    rows = [r + [""] * (3 - len(r)) for r in rows]
    head = ["Check", "Result", "Note"]
    w0 = max([len(head[0])] + [len(r[0]) for r in rows])
    w1 = max([len(head[1])] + [len(r[1]) for r in rows])
    lines = [f"{head[0]:<{w0}}  {head[1]:<{w1}}  {head[2]}", f"{'-' * w0}  {'-' * w1}  ----"]
    lines += [f"{c:<{w0}}  {s:<{w1}}  {n}".rstrip() for c, s, n in rows]
    count = {s: sum(r[1] == s for r in rows) for s in ("PASS", "FAIL", "SKIP", "INFO")}
    lines.append(", ".join(f"{n} {s.lower()}" for s, n in count.items() if n) or "no checks ran")
    return "\n".join(lines), count["FAIL"] == 0 and count["PASS"] > 0


def bridge(port, script):
    s = socket.create_connection(("127.0.0.1", port), timeout=30)
    s.sendall(script)
    s.shutdown(socket.SHUT_WR)
    return b"".join(iter(lambda: s.recv(65536), b"")).decode()


def port_free(port):
    """Nothing listens on the port, on 127.0.0.1 or ::1 (a dev server may take either). Sockets
    a closed run left in TIME_WAIT don't count: SO_REUSEADDR lets a bind past them."""
    for family, host in ((socket.AF_INET, "127.0.0.1"), (socket.AF_INET6, "::1")):
        try:
            with socket.socket(family) as c:
                c.settimeout(1)
                if c.connect_ex((host, port)) == 0:
                    return False
        except OSError:
            pass
    with socket.socket() as s:
        s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        try:
            s.bind(("127.0.0.1", port))
            return True
        except OSError:
            return False


def main(cmd, *args):
    stdin = lambda: sys.stdin.read()
    if cmd == "bridge":
        try:
            ok, value = bridge_value(bridge(int(args[0]), sys.stdin.buffer.read()))
        except OSError as e:
            ok, value = False, f"dev bridge: {e}"
        print(value, file=sys.stdout if ok else sys.stderr)
        return 0 if ok else 1
    if cmd == "eval-value":
        v = eval_value(stdin())
        print(v if isinstance(v, str) else json.dumps(v))
    elif cmd == "tab-state":
        print(tab_state(stdin(), args[0]))
    elif cmd == "tab-audio":
        print(tab_audio(stdin(), args[0]))
    elif cmd == "net-id":
        found = network_id(stdin(), args[0], args[1], int(args[2]))
        if found is None:
            return 1
        print(found)
    elif cmd == "pic-path":
        found = picture_path(stdin())
        if not found:
            return 1
        print(found)
    elif cmd in ("snapshot-ref", "popup-name", "file-path"):
        text = stdin()
        found = {"snapshot-ref": lambda: snapshot_ref(text, args[0]), "popup-name": lambda: popup_name(text),
                 "file-path": lambda: file_path(text, args[0])}[cmd]()
        if not found:
            return 1
        print(found)
    elif cmd == "closed-note":
        return 0 if closed_note(stdin(), args[0]) else 1
    elif cmd == "download":
        found = download(stdin())
        if not found:
            return 1
        print("\t".join(found))
    elif cmd == "skill-ours":
        try:
            with open(args[0], encoding="utf-8") as f:
                return 0 if skill_version(f.read()) else 1
        except (OSError, ValueError):
            return 1
    elif cmd == "agent":
        with open(args[0], encoding="utf-8", errors="replace") as f:
            ok, note = agent_verdict(agent_summary(f.read()), args[1])
        print(note)
        return 0 if ok else 1
    elif cmd == "form-submit":
        with open(args[0]) as f:
            ok, ua = form_submit(f.read(), args[1])
        print("from WebKit, not Chrome" if ok else f"user agent: {ua}" if ua else "no submit for this run")
        return 0 if ok else 1
    elif cmd == "json-str":
        print(json.dumps(args[0]))
    elif cmd == "nonce":
        import secrets
        print(secrets.token_hex(6))
    elif cmd == "shot-expect":
        print(*shot_expect(float(args[0]), float(args[1]), float(args[2]) if len(args) > 2 else 1.0))
    elif cmd == "png":
        with open(args[0], "rb") as f:
            size = png_size(f.read(24))
        if not size:
            return 1
        print(*size)
    elif cmd == "tall-sizes":
        # The pictures of the /tall page for a w×h viewport: piece = 2000 px of the page at most.
        w, h = float(args[0]), float(args[1])
        s = tall_scale(w, h)
        piece = int(2000 / s)
        heights = [min(piece, 3000 - top) for top in range(0, 3000, piece)]
        print(*(f"{round(w * s)}x{round(ph * s)}" for ph in heights))
    elif cmd == "tall-scale":
        print(tall_scale(float(args[0]), float(args[1])))
    elif cmd == "tall-check":
        scale, rows = float(args[0]), []
        for path in args[1:]:
            with open(path, "rb") as f:
                rows += png_column(f.read(), 10)
        fault = tall_check(rows, scale)
        print(fault or "the page once, top to bottom")
        return 1 if fault else 0
    elif cmd == "fps":
        print(f"{fps(json.loads(args[0]), json.loads(args[1])):.1f}")
    elif cmd == "missing":
        with open(args[0]) as f:
            print(", ".join(missing(f.read(), args[1:])))
    elif cmd == "memory":
        with open(args[0]) as a, open(args[1]) as b:
            print(memory_note(webcontent(a.read()), webcontent(b.read()), int(args[2])))
    elif cmd == "table":
        with open(args[0]) as f:
            text, ok = table([l.rstrip("\n").split("\t") for l in f if l.strip()])
        print(text)
        return 0 if ok else 1
    elif cmd == "port-free":
        return 0 if port_free(int(args[0])) else 1
    elif cmd == "free-port":
        with socket.socket() as s:
            s.bind(("127.0.0.1", 0))
            print(s.getsockname()[1])
    else:
        print(f"unknown command {cmd}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main(*sys.argv[1:]))
