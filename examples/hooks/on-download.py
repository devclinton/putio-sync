#!/usr/bin/env python3
"""Example putio-sync command hook: append each finished download to a log file.

putio-sync sends one event as JSON on stdin (see EVENTS.md). Hook it up with:

    [[hooks]]
    name = "on-download"
    events = ["download.finished"]
    command = ["/config/hooks/on-download.py"]
    env = { HOOK_LOG = "/config/hooks/downloads.log" }

Exit 0 for success; anything printed is kept in the daemon's log.
"""

import json
import os
import sys

event = json.load(sys.stdin)
data = event["data"]
log = os.environ.get("HOOK_LOG", "downloads.log")

if event["type"] == "hook.test":
    print(f"test event for {data['hook']} received")
    sys.exit(0)

with open(log, "a", encoding="utf-8") as out:
    out.write(f"{event['occurred_at']}\t{data['size_bytes']}\t{data['relative_path']}\n")
print(f"logged {data['name']}")
