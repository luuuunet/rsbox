#!/usr/bin/env python3
"""Dump TLS records after REALITY ClientHello (compare rsbox vs sing-box server)."""
import base64
import socket
import struct
import subprocess
import sys

HOST = sys.argv[1] if len(sys.argv) > 1 else "127.0.0.1"
PORT = int(sys.argv[2]) if len(sys.argv) > 2 else 8447


def get_hello():
    out = subprocess.check_output(
        ["cargo", "test", "-p", "rsb-protocol", "dump_reality_hello", "--", "--nocapture"],
        cwd=r"D:\morust\rsbox",
        stderr=subprocess.STDOUT,
        text=True,
    )
    for line in out.splitlines():
        if line.startswith("hello_b64="):
            return base64.b64decode(line.split("=", 1)[1])
    raise RuntimeError("hello not found")


def read_rec(s):
    hdr = s.recv(5)
    if len(hdr) < 5:
        return None
    ln = struct.unpack(">H", hdr[3:5])[0]
    body = b""
    while len(body) < ln:
        chunk = s.recv(ln - len(body))
        if not chunk:
            break
        body += chunk
    return hdr, body


def main():
    hello = get_hello()
    s = socket.create_connection((HOST, PORT), 10)
    s.sendall(hello)
    s.settimeout(5)
    for i in range(8):
        rec = read_rec(s)
        if not rec:
            print(f"rec{i}: EOF")
            break
        hdr, body = rec
        hs_type = body[0] if hdr[0] == 0x16 and body else None
        sid_len = None
        if hs_type == 2 and len(body) > 38:
            sid_len = body[38]
        print(
            f"rec{i}: type={hdr[0]:02x} ver={hdr[1]:03d}{hdr[2]} len={len(body)} "
            f"hs={hs_type} sid_len={sid_len}"
        )
    s.close()


if __name__ == "__main__":
    main()
