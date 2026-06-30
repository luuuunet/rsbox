#!/usr/bin/env python3
"""Capture REALITY server TLS records and attempt TLS 1.3 handshake decrypt."""
import base64
import hashlib
import hmac
import socket
import struct
import subprocess
import sys

from cryptography.hazmat.primitives.ciphers.aead import AESGCM
from cryptography.hazmat.primitives.asymmetric import x25519

HOST = sys.argv[1] if len(sys.argv) > 1 else "157.230.3.206"
PORT = int(sys.argv[2]) if len(sys.argv) > 2 else 8447


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


def read_rec(sock):
    hdr = sock.recv(5)
    if len(hdr) < 5:
        return None
    ln = struct.unpack(">H", hdr[3:5])[0]
    body = b""
    while len(body) < ln:
        chunk = sock.recv(ln - len(body))
        if not chunk:
            break
        body += chunk
    return hdr, body


def parse_server_hello(record):
    hs = record[5:]
    assert hs[0] == 0x02
    i = 4 + 2 + 32
    sess_len = hs[i]
    i += 1 + sess_len + 2 + 1
    cipher = struct.unpack(">H", hs[i - 2 : i])[0]
    ext_len = struct.unpack(">H", hs[i : i + 2])[0]
    i += 2
    end = i + ext_len
    server_pub = None
    while i + 4 <= end:
        et, el = struct.unpack(">HH", hs[i : i + 4])
        ed = hs[i + 4 : i + 4 + el]
        if et == 0x0033 and len(ed) >= 36:
            g, kl = struct.unpack(">HH", ed[:4])
            if g == 0x001D and kl == 32:
                server_pub = ed[4:36]
        i += 4 + el
    return cipher, server_pub, hs


def sha256(data):
    return hashlib.sha256(data).digest()


def sha384(data):
    return hashlib.sha384(data).digest()


def hkdf_extract(hash_fn, salt, ikm):
    return hmac.new(salt, ikm, hash_fn).digest()


def expand_label(hash_fn, secret, label, context, length):
    full = b"tls13 " + label.encode()
    hkdf_label = struct.pack(">H", length) + bytes([len(full)]) + full
    hkdf_label += bytes([len(context)]) + context
    out = b""
    t = b""
    i = 1
    while len(out) < length:
        t = hmac.new(secret, t + hkdf_label + bytes([i]), hash_fn).digest()
        out += t
        i += 1
    return out[:length]


def derive_keys(cipher, shared, transcript):
    if cipher == 0x1302:
        hash_fn = hashlib.sha384
        empty = sha384(b"")
        zlen = 48
        klen = 32
    else:
        hash_fn = hashlib.sha256
        empty = sha256(b"")
        zlen = 32
        klen = 16
    zero = b"\x00" * zlen
    early = hkdf_extract(hash_fn, empty, zero)
    derived = expand_label(hash_fn, early, "derived", b"", zlen)
    hs_secret = hkdf_extract(hash_fn, derived, shared)
    th = transcript
    server_secret = expand_label(hash_fn, hs_secret, "s hs traffic", th, 32)
    read_key = expand_label(hash_fn, server_secret, "key", b"", klen)
    read_iv = expand_label(hash_fn, server_secret, "iv", b"", 12)
    return read_key, read_iv, hash_fn


def record_nonce(base_iv, seq):
    nonce = bytearray(base_iv)
    seq_bytes = struct.pack(">Q", seq)
    for i in range(8):
        nonce[4 + i] ^= seq_bytes[i]
    return bytes(nonce)


def decrypt_record(read_key, read_iv, seq, hdr, enc):
    nonce = record_nonce(read_iv, seq)
    return AESGCM(read_key).decrypt(nonce, enc, hdr)


def main():
    hello, secret = cargo_hello()
    transcript = hello[5:]
    s = socket.create_connection((HOST, PORT), 15)
    s.sendall(hello)
    s.settimeout(10)

    sh = read_rec(s)
    print("server_hello type", sh[0][0], "len", len(sh[1]))
    cipher, server_pub, sh_hs = parse_server_hello(sh[0] + sh[1])
    print("cipher", hex(cipher), "server_pub", server_pub[:8].hex() if server_pub else None)
    transcript += sh_hs

    shared = x25519.X25519PrivateKey.from_private_bytes(secret).exchange(
        x25519.X25519PublicKey.from_public_bytes(server_pub)
    )
    th = sha384(transcript) if cipher == 0x1302 else sha256(transcript)
    read_key, read_iv, _ = derive_keys(cipher, shared, th)

    seq = 0
    for i in range(6):
        rec = read_rec(s)
        if not rec:
            break
        hdr, body = rec
        print(f"rec{i} type={hdr[0]:#x} len={len(body)}")
        if hdr[0] == 0x14:
            continue
        try:
            plain = decrypt_record(read_key, read_iv, seq, hdr, body)
            seq += 1
            print(f"  decrypt_ok len={len(plain)} last_type={plain[-1]:#x}")
        except Exception as e:
            print(f"  decrypt_fail {e}")
    s.close()


if __name__ == "__main__":
    main()
