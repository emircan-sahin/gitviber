"""Test pages for browser-check.sh: python3 -I browser-check-site.py <port>. Logs what the checks
look for (an SSE stream the client left) to stdout.

  /       an rAF counter (window.frames_), a CSS animation, a localStorage "login", and a fetch
          (/api/hello) and an XHR (/api/missing, 404) as it loads; startSse() and stopSse()
  /form   a text field, a password field and a <select>, posted to /api/form
  /sse    an event a half second, for five minutes
  /act    a button whose fetch gets a 500, one that does nothing, a "Sign in" link, a popup
          opener that keeps what the popup posts (window.msg_), and a download link
  /covered, /smooth, /fill, /native, /frames?other=<port>, /inner, /popup: one page per check
  /file   a text file served as an attachment"""
import json
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs

PAGE = """<!doctype html><html><head><title>live-check</title><style>
@keyframes spin { from { transform: rotate(0deg) } to { transform: rotate(360deg) } }
#box { width: 60px; height: 60px; background: tomato; animation: spin 2s linear infinite; }
</style></head><body>
<h1 id="who">anon</h1><div id="box"></div>
<button id="login" onclick="login()">Log in</button>
<script>
window.frames_ = 0;
(function tick() { window.frames_++; requestAnimationFrame(tick); })();
function show() { document.getElementById('who').textContent = localStorage.getItem('token') || 'anon'; }
async function login() {
  const r = await fetch('/api/login', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ user: 'alice' }) });
  const j = await r.json(); localStorage.setItem('token', j.token); show();
}
function startSse() { window.es = new EventSource('/sse'); window.events_ = 0; es.onmessage = () => window.events_++; }
function stopSse() { window.es.close(); }
show();
fetch('/api/hello?x=1').then(r => r.json());
const x = new XMLHttpRequest(); x.open('GET', '/api/missing'); x.send();
</script></body></html>"""

FORM = """<!doctype html><html><head><title>live-check form</title></head><body>
<form id="f">
  <label>Name <input id="name" name="name"></label>
  <label>Password <input id="pw" name="pw" type="password" autocomplete="off"></label>
  <label>Colour <select id="color" name="color"><option value="red">Red</option><option value="green">Green</option></select></label>
  <button id="send" type="submit">Send</button>
</form>
<p id="out"></p>
<script>
document.getElementById('f').addEventListener('submit', async (e) => {
  e.preventDefault();
  const r = await fetch('/api/form', { method: 'POST', body: new URLSearchParams(new FormData(e.target)) });
  const j = await r.json();
  document.getElementById('out').textContent = `sent ${j.name} ${j.color} pw:${j.pw ? 'yes' : 'no'}`;
});
</script></body></html>"""


def page(title, body, head=""):
    return f"<!doctype html><html><head><title>{title}</title>{head}</head><body>{body}</body></html>"


PAGES = {
    "/act": page("act", """
<h1 id="who" style="color: rgb(0, 128, 0)">Act</h1>
<button id="boom" onclick="fetch('/api/x')">Boom</button>
<button id="noop">Nothing</button>
<a id="signin" href="#">Sign in</a>
<button id="pop" onclick="window.open('/popup')">Open popup</button>
<a id="dl" href="/file">Download</a>
<script>window.addEventListener('message', (e) => (window.msg_ = e.data));</script>"""),
    "/covered": page("covered", """
<button id="under" style="position: absolute; top: 10px; left: 10px">Under</button>
<div id="veil" style="position: fixed; top: 0; left: 0; width: 300px; height: 80px; background: rgba(0, 0, 0, 0.3)"></div>
<fieldset disabled style="margin-top: 120px"><button id="off">Off</button></fieldset>"""),
    "/smooth": page("smooth", """
<div style="height: 4000px">top</div>
<button id="far" onclick="document.title = 'far clicked'">Far</button>""",
        "<style>html { scroll-behavior: smooth }</style>"),
    "/fill": page("fill", """
<input id="short" maxlength="4">
<input id="cb" type="checkbox"> <label for="cb">Agree</label>
<input id="phone">
<script>
// A mask as sign-up forms have: digits only, shown as (555) 123-4567.
document.getElementById('phone').addEventListener('input', (e) => {
  const d = e.target.value.replace(/\\D/g, '').slice(0, 10);
  e.target.value = d.length > 6 ? `(${d.slice(0, 3)}) ${d.slice(3, 6)}-${d.slice(6)}` : d;
});
</script>"""),
    "/native": page("native", """
<button id="trust" onclick="document.title = String(event.isTrusted)">Trust</button>
<p id="hov">Hover me</p>
<a id="lnk" href="#" onclick="event.preventDefault(); document.title = 'shift:' + event.shiftKey">Link</a>
<input id="txt">
<button id="copy" onclick="copyOut()">Copy</button>
<textarea id="src">gitviber-check-copied</textarea>
<script>
window.keyGap_ = null;
let down = 0;
document.addEventListener('keydown', (e) => { if (e.key === 'w' && !e.repeat) down = performance.now(); });
document.addEventListener('keyup', (e) => { if (e.key === 'w') window.keyGap_ = performance.now() - down; });
function copyOut() {
  navigator.clipboard?.writeText('gitviber-check-copied').catch(() => {});
  const t = document.getElementById('src'); t.select(); document.execCommand('copy');
}
</script>""", "<style>#hov { color: rgb(0, 0, 0) } #hov:hover { color: rgb(255, 0, 0) }</style>"),
    "/inner": page("inner", """
<button id="inner" onclick="document.title = 'inner clicked'">Inner</button>
<button id="oops" onclick="throw new Error('inner oops')">Oops</button>"""),
    "/popup": page("popup", """
<p>Popup</p><button id="closeme" onclick="window.close()">Close</button>
<script>window.opener && window.opener.postMessage('hi-from-popup', '*');</script>"""),
}


class H(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def send(self, code, body, ctype):
        b = body.encode()
        self.send_response(code)
        self.send_header("content-type", ctype)
        self.send_header("content-length", str(len(b)))
        self.end_headers()
        self.wfile.write(b)

    def do_GET(self):
        path = self.path.split("?")[0]
        if path == "/":
            self.send(200, PAGE, "text/html")
        elif path == "/form":
            self.send(200, FORM, "text/html")
        elif path in PAGES:
            self.send(200, PAGES[path], "text/html")
        elif path == "/frames":
            other = parse_qs(self.path.partition("?")[2]).get("other", ["0"])[0]
            body = ('<p>Frames</p><iframe id="same" src="/inner"></iframe>'
                    f'<iframe id="cross" src="http://127.0.0.1:{int(other)}/inner"></iframe>')
            self.send(200, page("frames", body), "text/html")
        elif path == "/file":
            b = b"gitviber check download\n"
            self.send_response(200)
            self.send_header("content-type", "text/plain")
            self.send_header("content-disposition", 'attachment; filename="gitviber-check.txt"')
            self.send_header("content-length", str(len(b)))
            self.end_headers()
            self.wfile.write(b)
        elif path == "/api/x":
            self.send(500, json.dumps({"error": "boom"}), "application/json")
        elif path == "/api/hello":
            self.send(200, json.dumps({"hello": "world"}), "application/json")
        elif path == "/sse":
            self.send_response(200)
            self.send_header("content-type", "text/event-stream")
            self.send_header("cache-control", "no-store")
            self.end_headers()
            print("sse: client came", flush=True)
            try:
                for i in range(600):
                    self.wfile.write(f"data: {i}\n\n".encode())
                    self.wfile.flush()
                    time.sleep(0.5)
            except (BrokenPipeError, ConnectionResetError):
                print("sse: client went away", flush=True)
        else:
            self.send(404, json.dumps({"error": "nope"}), "application/json")

    def do_POST(self):
        n = int(self.headers.get("content-length", 0))
        raw = self.rfile.read(n)
        if self.path == "/api/form":
            form = {k: v[0] for k, v in parse_qs(raw.decode()).items()}
            # Never echoed: whether there was one is all the page needs.
            reply = {"name": form.get("name", ""), "color": form.get("color", ""), "pw": bool(form.get("pw"))}
        else:
            reply = {"token": f"tok-{json.loads(raw or b'{}').get('user')}"}
        self.send(200, json.dumps(reply), "application/json")


ThreadingHTTPServer(("127.0.0.1", int(sys.argv[1])), H).serve_forever()
