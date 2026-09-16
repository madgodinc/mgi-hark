#!/usr/bin/env python3
"""Read what installed copies sent.  python3 show.py [errors|feedback] [N]"""

import sqlite3
import sys
import time

DB = "/data/hark-api/hark.db"
what = sys.argv[1] if len(sys.argv) > 1 else "feedback"
n = int(sys.argv[2]) if len(sys.argv) > 2 else 20

conn = sqlite3.connect(DB)
if what == "errors":
    rows = conn.execute(
        "SELECT at, version, os, kind, message, COUNT(*) FROM errors GROUP BY kind, substr(message,1,160) ORDER BY MAX(at) DESC LIMIT ?",
        (n,),
    )
    for at, version, os_, kind, message, count in rows:
        print(f"{time.strftime('%Y-%m-%d %H:%M', time.localtime(at))}  x{count}  [{kind}] {version} {os_}\n  {message}\n")
else:
    rows = conn.execute("SELECT id, at, version, category, message, contact, length(report) FROM feedback ORDER BY id DESC LIMIT ?", (n,))
    for id_, at, version, category, message, contact, report in rows:
        print(f"#{id_} {time.strftime('%Y-%m-%d %H:%M', time.localtime(at))} {category} v{version} contact: {contact or '-'} report: {report or 0} chars\n  {message}\n")
