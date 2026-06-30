#!/usr/bin/env python3
"""Run REALITY handshake debug from server localhost."""
import base64
import hashlib
import hmac
import socket
import struct
import subprocess
import sys

import paramiko
from cryptography.hazmat.primitives.ciphers.aead import AESGCM
from cryptography.hazmat.primitives.asymmetric import x25519

HOST = "157.230.3.206"
USER = "root"
PASSWORD = "ZqF4THu3x9f"


def cargo_hello():
    out = subprocess.check_output(
        ["cargo", "test", "-p", "rsb-protocol", "dump_reality_hello", "--", "--nocapture"],
        cwd=r"D:\morust\rsbox",
        stderr=subprocess.STDOUT,
        text=True,
    )
    hello_b64 = secret_b64 = None
    for line in out.splitlines():
        if line.startswith("hello_b64="):
            hello_b64 = line.split("=", 1)[1]
        if line.startswith("secret_b64="):
            secret_b64 = line.split("=", 1)[1]
    return base64.b64decode(hello_b64), base64.b64decode(secret_b64)


def main():
    hello, secret = cargo_hello()
    remote = f'''
import base64, hashlib, hmac, socket, struct
from cryptography.hazmat.primitives.ciphers.aead import AESGCM
from cryptography.hazmat.primitives.asymmetric import x25519

hello = base64.b64decode("{base64.b64encode(hello).decode()}")
secret = base64.b64decode("{base64.b64encode(secret).decode()}")

def read_rec(s):
    hdr = s.recv(5)
    if len(hdr) < 5: return None
    ln = struct.unpack(">H", hdr[3:5])[0]
    body = b""
    while len(body) < ln:
        body += s.recv(ln - len(body))
    return hdr, body

def parse_sh(rec):
    hs = rec[5:]
    if hs[0] != 2:
        return None, None, hs
    i = 4 + 2 + 32
    sl = hs[i]; i += 1 + sl + 2 + 1
    cipher = struct.unpack(">H", hs[i-2:i])[0]
    el = struct.unpack(">H", hs[i:i+2])[0]; i += 2
    end = i + el
    pub = None
    while i + 4 <= end:
        et, xl = struct.unpack(">HH", hs[i:i+4])
        ed = hs[i+4:i+4+xl]
        if et == 0x33 and len(ed) >= 36:
            g, kl = struct.unpack(">HH", ed[:4])
            if g == 0x001d and kl == 32:
                pub = ed[4:36]
        i += 4 + xl
    return cipher, pub, hs

def sha256(d): return hashlib.sha256(d).digest()
def sha384(d): return hashlib.sha384(d).digest()
def hkdf_extract(hf, salt, ikm): return hmac.new(salt, ikm, hf).digest()
def expand_label(hf, sec, label, ctx, ln):
    full = b"tls13 " + label.encode()
    lab = struct.pack(">H", ln) + bytes([len(full)]) + full + bytes([len(ctx)]) + ctx
    out = b""; t = b""; i = 1
    while len(out) < ln:
        t = hmac.new(sec, t + lab + bytes([i]), hf).digest(); out += t; i += 1
    return out[:ln]

def derive(cipher, shared, transcript):
    if cipher == 0x1302:
        hf = hashlib.sha384; empty = sha384(b""); zlen = 48; klen = 32
    else:
        hf = hashlib.sha256; empty = sha256(b""); zlen = 32; klen = 16
    zero = b"\\x00" * zlen
    early = hkdf_extract(hf, empty, zero)
    derived = expand_label(hf, early, "derived", b"", zlen)
    hs = hkdf_extract(hf, derived, shared)
    th = sha384(transcript) if cipher == 0x1302 else sha256(transcript)
    ss = expand_label(hf, hs, "s hs traffic", th, 32)
    return expand_label(hf, ss, "key", b"", klen), expand_label(hf, ss, "iv", b"", 12), hf

def nonce(iv, seq):
    n = bytearray(iv); sb = struct.pack(">Q", seq)
    for j in range(8): n[4+j] ^= sb[j]
    return bytes(n)

transcript = hello[5:]
s = socket.create_connection(("127.0.0.1", 8447), 10)
s.sendall(hello); s.settimeout(8)
sh = read_rec(s)
print("sh_type", sh[0][0], "sh_len", len(sh[1]))
if sh[0][0] == 21:
    print("alert", sh[1].hex()); s.close(); raise SystemExit
cipher, pub, sh_hs = parse_sh(sh[0]+sh[1])
print("cipher", hex(cipher) if cipher else None, "pub", pub[:8].hex() if pub else None)
transcript += sh_hs
shared = x25519.X25519PrivateKey.from_private_bytes(secret).exchange(x25519.X25519PublicKey.from_public_bytes(pub))
rk, riv, hf = derive(cipher, shared, transcript)
seq = 0
for i in range(6):
    rec = read_rec(s)
    if not rec: break
    hdr, body = rec
    print("rec", i, hex(hdr[0]), len(body))
    if hdr[0] == 0x14: continue
    try:
        pt = AESGCM(rk).decrypt(nonce(riv, seq), body, hdr)
        seq += 1
        print("  ok", len(pt), hex(pt[-1]))
    except Exception as e:
        print("  fail", e)
s.close()
'''
    c = paramiko.SSHClient()
    c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    c.connect(HOST, username=USER, password=PASSWORD, timeout=30)
    _, o, e = c.exec_command(f"python3 - <<'PY'\n{remote}\nPY", timeout=60)
    print(o.read().decode())
    err = e.read().decode()
    if err.strip():
        print("ERR", err[:500])
    c.close()


if __name__ == "__main__":
    main()
