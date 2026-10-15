"""Test pages for browser-check.sh: python3 -I browser-check-site.py <port>. Logs what the checks
look for (an SSE stream the client left) to stdout.

  /       an rAF counter (window.frames_), a CSS animation, a localStorage "login", and a fetch
          (/api/hello) and an XHR (/api/missing, 404) as it loads; startSse() and stopSse()
  /form   a text field, a password field and a <select>, posted to /api/form
  /sse    an event a half second, for five minutes"""
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
