"""Serve live_probe output as SSE so the /overlay and /reader prototypes show real Live Translate lines.

usage: python3 tools/live_bridge.py <live_probe log.tsv> [port=8791]
then open overlay.html?live=http://localhost:8791/events
events: `line` {id, en, th} (sent again as the open line grows), `gone` {id} when the line closes.
Line rule = tools/lines.py: close on the first empty TH tick, or once EN ends a sentence with 40+ chars
and the next non-empty EN fragment arrives.
"""
import json
import re
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

LOG = sys.argv[1]
PORT = int(sys.argv[2]) if len(sys.argv) > 2 else 8791


def events(f):
    """Yield (event, data) from new log rows, forever."""
    en, th, ends, n = "", "", False, 0
    while True:
        raw = f.readline()
        if not raw:
            time.sleep(0.1)
            continue
        c = raw.rstrip("\n").split("\t")
        if len(c) < 4 or c[1] not in ("en", "th"):
            continue
        kind, text = c[1], c[2]
        if not text.strip():
            if kind == "th" and (en or th):
                yield "gone", {"id": n}
                en, th, ends, n = "", "", False, n + 1
            continue
        if kind == "en" and ends and len(en) >= 40:
            yield "gone", {"id": n}
            en, th, ends, n = "", "", False, n + 1
        if kind == "en":
            en += text
            ends = bool(re.search(r"[.?!]\s*$", en))
        else:
            th += text
        yield "line", {"id": n, "en": en.strip(), "th": th.strip()}


class H(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path != "/events":
            self.send_error(404)
            return
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Cache-Control", "no-cache")
        self.send_header("Access-Control-Allow-Origin", "*")
        self.end_headers()
        with open(LOG) as f:
            f.seek(0, 2)  # only new lines
            try:
                for ev, data in events(f):
                    self.wfile.write(f"event: {ev}\ndata: {json.dumps(data, ensure_ascii=False)}\n\n".encode())
                    self.wfile.flush()
            except (BrokenPipeError, ConnectionResetError):
                pass

    def log_message(self, *a):
        pass


ThreadingHTTPServer(("0.0.0.0", PORT), H).serve_forever()
