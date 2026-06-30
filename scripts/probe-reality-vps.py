#!/usr/bin/env python3
"""Tail VPS rsbox logs while running a local reality client test."""
import json
import os
import subprocess
import sys
import threading
import time

import paramiko

HOST = "157.230.3.206"
USER = "root"
PASSWORD = "ZqF4THu3x9f"
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
RSBOX = os.path.join(ROOT, "target", "release", "rsbox.exe")
CFG = os.path.join(ROOT, "examples", "generated", "protocol-tests", "reality.json")


def tail_logs(stop: threading.Event):
    c = paramiko.SSHClient()
    c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    c.connect(HOST, username=USER, password=PASSWORD, timeout=30)
    _, stdout, _ = c.exec_command(
        "journalctl -u rsbox -f -n 0 --no-pager 2>/dev/null", timeout=120
    )
    try:
        while not stop.is_set():
            line = stdout.readline()
            if not line:
                break
            if any(k in line for k in ("reality", "vless", "passthrough", "session")):
                print(f"[VPS] {line.rstrip()}")
    finally:
        c.close()


def main():
    if not os.path.isfile(RSBOX):
        print("build rsbox first", file=sys.stderr)
        sys.exit(1)
    stop = threading.Event()
    t = threading.Thread(target=tail_logs, args=(stop,), daemon=True)
    t.start()
    time.sleep(1)
    subprocess.run(["taskkill", "/F", "/IM", "rsbox.exe"], capture_output=True)
    log = os.path.join(os.environ.get("TEMP", "/tmp"), "rsbox-reality-probe.log")
    p = subprocess.Popen(
        [RSBOX, "run", "-c", CFG],
        stderr=open(log, "w", encoding="utf-8"),
        stdout=subprocess.DEVNULL,
    )
    time.sleep(3)
    print("[local] curling via proxy...")
    r = subprocess.run(
        [
            "curl.exe",
            "-x",
            "http://127.0.0.1:17891",
            "-sS",
            "-o",
            "NUL",
            "-w",
            "%{http_code}",
            "--connect-timeout",
            "20",
            "--max-time",
            "35",
            "https://1.1.1.1/cdn-cgi/trace",
        ],
        capture_output=True,
        text=True,
    )
    print(f"[local] curl exit={r.returncode} out={r.stdout.strip()} err={r.stderr.strip()[:200]}")
    stop.set()
    p.terminate()
    subprocess.run(["taskkill", "/F", "/IM", "rsbox.exe"], capture_output=True)
    if os.path.isfile(log):
        print("--- local rsbox tail ---")
        with open(log, encoding="utf-8", errors="replace") as f:
            lines = f.readlines()
        for line in lines[-15:]:
            print(line.rstrip())


if __name__ == "__main__":
    main()
