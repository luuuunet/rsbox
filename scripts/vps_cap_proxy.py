#!/usr/bin/env python3
"""VPS: capture sing-box ClientHello and rsbox server flight to /tmp."""
import base64
import json
import socket
import struct
import subprocess
import sys
import time

HELLO_PATH = "/tmp/sb_hello.bin"
FLIGHT_PATH = "/tmp/sb_flight.bin"


def forward(hello: bytes):
    s = socket.create_connection(("127.0.0.1", 8447), 8)
    s.sendall(hello)
    flight = b""
    for _ in range(8):
        hdr = s.recv(5)
        if len(hdr) < 5:
            break
        ln = struct.unpack(">H", hdr[3:5])[0]
        body = b""
        while len(body) < ln:
            body += s.recv(ln - len(body))
        flight += hdr + body
        if hdr[0] == 0x17:
            break
    s.close()
    return flight


def proxy_once():
    ls = socket.socket()
    ls.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    ls.bind(("127.0.0.1", 18447))
    ls.listen(1)
    ls.settimeout(35)
    c, addr = ls.accept()
    print(f"accepted {addr}", flush=True)
    data = b""
    while len(data) < 5:
        chunk = c.recv(65536)
        if not chunk:
            c.close()
            ls.close()
            return False
        data += chunk
    ln = struct.unpack(">H", data[3:5])[0]
    while len(data) < 5 + ln:
        chunk = c.recv(65536)
        if not chunk:
            c.close()
            ls.close()
            return False
        data += chunk
    open(HELLO_PATH, "wb").write(data)
    print(f"hello_len={len(data)}", flush=True)
    flight = forward(data)
    open(FLIGHT_PATH, "wb").write(flight)
    print(f"flight_len={len(flight)}", flush=True)
    c.close()
    ls.close()
    print("HELLO_B64=" + base64.b64encode(data).decode(), flush=True)
    return True


def main():
    ok = proxy_once()
    if not ok:
        print("capture failed")
        sys.exit(1)
    # show record breakdown
    flight = open(FLIGHT_PATH, "rb").read()
    i = 0
    recs = []
    while i + 5 <= len(flight):
        hdr = flight[i : i + 5]
        ln = struct.unpack(">H", hdr[3:5])[0]
        recs.append({"type": hdr[0], "len": ln})
        i += 5 + ln
    print("flight_recs=" + json.dumps(recs))


if __name__ == "__main__":
    main()
