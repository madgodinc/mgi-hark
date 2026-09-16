#!/usr/bin/env python3
"""Hark reports: error reports and support messages from installed copies.

Runs on tyan behind Caddy (madgodinc.net/hark/api/*), listening on 127.0.0.1
only. Standard library, no dependencies.

  POST /hark/api/error     {install, version, os, kind, message, detail}
  POST /hark/api/feedback  {install, version, os, category, message, contact, report}

Stored in SQLite. Every support message and the first occurrence of each
distinct error in a day are forwarded to the owner on Telegram.

Nothing here is trusted: sizes are capped, fields are length-limited and
stripped of control characters, and each address gets a small hourly quota.
The app never sends caption text; the server still refuses to store more than
the capped fields.
"""

import json
import os
import re
import sqlite3
import threading
import time
import urllib.parse
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

HOST = os.environ.get("HARK_API_HOST", "127.0.0.1")
PORT = int(os.environ.get("HARK_API_PORT", "8795"))
DB = os.environ.get("HARK_API_DB", "/data/hark-api/hark.db")
TG_TOKEN = os.environ.get("HARK_TG_BOT_TOKEN", "")
TG_CHAT = os.environ.get("HARK_TG_CHAT", "")

MAX_BODY = {"error": 16 * 1024, "feedback": 96 * 1024}
QUOTA_PER_HOUR = {"error": 60, "feedback": 12}
FIELD = {
    "install": 64, "version": 32, "os": 120, "kind": 48, "message": 2000,
    "detail": 8000, "category": 24, "contact": 200, "report": 60000,
}
CATEGORIES = {"problem": "Проблема", "idea": "Идея", "other": "Другое"}

CONTROL = re.compile(r"[\x00-\x08\x0b\x0c\x0e-\x1f\x7f]")
lock = threading.Lock()
hits: dict[tuple[str, str], list[float]] = {}


def clean(value, limit):
    if value is None:
        return ""
    return CONTROL.sub("", str(value))[:limit].strip()


def db():
    conn = sqlite3.connect(DB, timeout=10)
    conn.execute("PRAGMA journal_mode=WAL")
    return conn


def init_db():
    with db() as conn:
        conn.executescript(
            """
            CREATE TABLE IF NOT EXISTS errors (
              id INTEGER PRIMARY KEY, at INTEGER NOT NULL, install TEXT, version TEXT,
              os TEXT, kind TEXT, message TEXT, detail TEXT, ip TEXT);
            CREATE INDEX IF NOT EXISTS errors_at ON errors(at);
            CREATE TABLE IF NOT EXISTS feedback (
              id INTEGER PRIMARY KEY, at INTEGER NOT NULL, install TEXT, version TEXT,
              os TEXT, category TEXT, message TEXT, contact TEXT, report TEXT, ip TEXT);
            """
        )


def allowed(ip, route):
    now = time.time()
    with lock:
        recent = [t for t in hits.get((ip, route), []) if now - t < 3600]
        if len(recent) >= QUOTA_PER_HOUR[route]:
            hits[(ip, route)] = recent
            return False
        recent.append(now)
        hits[(ip, route)] = recent
        return True


def telegram(text):
    if not TG_TOKEN or not TG_CHAT:
        return
    data = urllib.parse.urlencode({"chat_id": TG_CHAT, "text": text[:4000], "disable_web_page_preview": "true"}).encode()
    try:
        urllib.request.urlopen(f"https://api.telegram.org/bot{TG_TOKEN}/sendMessage", data=data, timeout=10)
    except Exception as exc:  # best effort: a lost notification never loses the stored report
        print(f"telegram failed: {exc}", flush=True)


def notify_async(text):
    threading.Thread(target=telegram, args=(text,), daemon=True).start()


class Handler(BaseHTTPRequestHandler):
    server_version = "hark-api"

    def log_message(self, fmt, *args):
        print(f"{self.client_ip()} {fmt % args}", flush=True)

    def client_ip(self):
        return self.headers.get("X-Real-IP") or self.client_address[0]

    def reply(self, code, body):
        raw = json.dumps(body).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def do_GET(self):
        if self.path == "/hark/api/health":
            return self.reply(200, {"ok": True})
        self.reply(404, {"error": "not found"})

    def do_POST(self):
        route = {"/hark/api/error": "error", "/hark/api/feedback": "feedback"}.get(self.path)
        if not route:
            return self.reply(404, {"error": "not found"})
        try:
            length = int(self.headers.get("Content-Length", "0"))
        except ValueError:
            return self.reply(400, {"error": "bad length"})
        if length <= 0 or length > MAX_BODY[route]:
            return self.reply(413, {"error": "too large"})
        ip = self.client_ip()
        if not allowed(ip, route):
            return self.reply(429, {"error": "too many"})
        try:
            data = json.loads(self.rfile.read(length))
            if not isinstance(data, dict):
                raise ValueError
        except (ValueError, json.JSONDecodeError):
            return self.reply(400, {"error": "bad json"})

        f = {k: clean(data.get(k), n) for k, n in FIELD.items()}
        now = int(time.time())
        if route == "error":
            if not f["message"]:
                return self.reply(400, {"error": "empty"})
            with db() as conn:
                seen = conn.execute(
                    "SELECT COUNT(*) FROM errors WHERE kind=? AND substr(message,1,160)=substr(?,1,160) AND at>?",
                    (f["kind"], f["message"], now - 86400),
                ).fetchone()[0]
                conn.execute(
                    "INSERT INTO errors(at,install,version,os,kind,message,detail,ip) VALUES(?,?,?,?,?,?,?,?)",
                    (now, f["install"], f["version"], f["os"], f["kind"], f["message"], f["detail"], ip),
                )
            if seen == 0:
                notify_async(f"Hark: новая ошибка ({f['kind'] or 'unknown'})\n{f['message']}\n\nВерсия {f['version']}, {f['os']}")
            return self.reply(200, {"ok": True})

        if not f["message"]:
            return self.reply(400, {"error": "empty"})
        category = f["category"] if f["category"] in CATEGORIES else "other"
        with db() as conn:
            cur = conn.execute(
                "INSERT INTO feedback(at,install,version,os,category,message,contact,report,ip) VALUES(?,?,?,?,?,?,?,?,?)",
                (now, f["install"], f["version"], f["os"], category, f["message"], f["contact"], f["report"], ip),
            )
            number = cur.lastrowid
        notify_async(
            f"Hark, поддержка №{number}: {CATEGORIES[category]}\n\n{f['message']}\n\n"
            f"Контакт: {f['contact'] or 'не указан'}\nВерсия {f['version']}, {f['os']}"
            + ("\nПриложен технический отчёт" if f["report"] else "")
        )
        self.reply(200, {"ok": True, "number": number})


if __name__ == "__main__":
    init_db()
    print(f"hark-api on {HOST}:{PORT}", flush=True)
    ThreadingHTTPServer((HOST, PORT), Handler).serve_forever()
