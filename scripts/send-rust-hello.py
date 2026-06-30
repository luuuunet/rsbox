#!/usr/bin/env python3
"""Send exact Rust-generated REALITY ClientHello to server."""
import base64
import socket
import subprocess
import sys


def get_hello_b64() -> bytes:
    out = subprocess.check_output(
        ["cargo", "test", "-p", "rsb-protocol", "dump_reality_hello", "--", "--nocapture"],
        cwd=r"D:\morust\rsbox",
        stderr=subprocess.STDOUT,
        text=True,
    )
    for line in out.splitlines():
        if line.startswith("hello_b64="):
            return base64.b64decode(line.split("=", 1)[1])
    raise RuntimeError("hello_b64 not found")


def main():
    host = sys.argv[1] if len(sys.argv) > 1 else "157.230.3.206"
    port = int(sys.argv[2]) if len(sys.argv) > 2 else 8447
    hello = get_hello_b64()
    s = socket.create_connection((host, port), 10)
    s.sendall(hello)
    s.settimeout(5)
    resp = s.recv(4096)
    s.close()
    print(f"sent {len(hello)} to {host}:{port}, recv {len(resp)}")
    if resp:
        print(f"record type {resp[0]:#x}")
        if resp[0] == 0x16 and len(resp) > 5:
            print(f"hs type {resp[5]:#x}")


if __name__ == "__main__":
    main()
