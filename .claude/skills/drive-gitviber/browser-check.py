"""The parts of browser-check.sh that parse or print, so they can be tested apart from a running
app. Run with `python3 -I browser-check.py <command> ...`; replies to parse come on stdin."""
import json
import re
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


def png_size(data):
    """A PNG's width and height from its IHDR."""
    if data[:8] != b"\x89PNG\r\n\x1a\n" or data[12:16] != b"IHDR":
        return None
    return struct.unpack(">II", data[16:24])


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
    elif cmd == "shot-expect":
        print(*shot_expect(float(args[0]), float(args[1]), float(args[2]) if len(args) > 2 else 1.0))
    elif cmd == "png":
        with open(args[0], "rb") as f:
            size = png_size(f.read(24))
        if not size:
            return 1
        print(*size)
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
