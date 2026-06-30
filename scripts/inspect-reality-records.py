#!/usr/bin/env python3
"""Inspect TLS records after REALITY ServerHello."""
import base64
import socket
import struct
import subprocess
import time
from cryptography.hazmat.primitives.asymmetric import x25519
from cryptography.hazmat.primitives.ciphers.aead import AESGCM
from cryptography.hazmat.primitives.kdf.hkdf import HKDF
from cryptography.hazmat.primitives import hashes
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat

HOST = "157.230.3.206"
PORT = 8447
PRIV = "SBmx3gF9whM-5Df2zEz2dBops2Dw6gIDakZ_-dRrDHw"
PUB = "VdvA1In4Po7ugbQHYIm518Vw5u72SFokjyTY4XwByRw"


def build_and_patch():
    # reuse cargo-generated hello via subprocess would need rust; build inline minimal with fixes
    import os
    priv = x25519.X25519PrivateKey.generate()
    pub = priv.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
    server_pub = __import__("base64").urlsafe_b64decode(PUB + "==")
    shared = priv.exchange(x25519.X25519PublicKey.from_public_bytes(server_pub))
    random = os.urandom(32)
    session_plain = bytearray(16)
    session_plain[0:3] = bytes([1, 8, 1])
    struct.pack_into(">I", session_plain, 4, int(time.time()))
    session_plain[8:12] = bytes.fromhex("a1b2c3d4")

    def build_hello():
        sni = b"www.cloudflare.com"
        exts = b""
        sni_ext = struct.pack(">H", len(sni) + 3) + b"\x00" + struct.pack(">H", len(sni)) + sni
        exts += struct.pack(">HH", 0, len(sni_ext)) + sni_ext
        exts += struct.pack(">HH", 5) + bytes([4, 3, 4, 3, 3])
        ks = struct.pack(">HH", 0x001D, 32) + pub
        exts += struct.pack(">HH", 51, len(ks) + 2) + struct.pack(">H", len(ks)) + ks
        exts += struct.pack(">HHH", 43, 2, 0x0304)
        body = b"\x03\x03" + random + bytes([32]) + bytes(32)
        body += struct.pack(">H", 2) + struct.pack(">H", 0x1301)
        body += b"\x01\x00" + struct.pack(">H", len(exts)) + exts
        hs = b"\x01" + struct.pack(">I", len(body))[1:] + body
        return bytearray(b"\x16\x03\x01" + struct.pack(">H", len(hs)) + hs)

    hello = build_hello()
    raw = hello[5:]
    sid_off = 39
    aad = bytes(bytearray(raw)[:sid_off] + b"\x00" * 32 + bytearray(raw)[sid_off + 32 :])
    auth = HKDF(hashes.SHA256(), 32, random[:20], b"REALITY").derive(shared)
    sealed = AESGCM(auth).encrypt(random[20:32], bytes(session_plain), aad)
    hello[sid_off : sid_off + 32] = sealed
    return bytes(hello)


def main():
    hello = build_and_patch()
    s = socket.create_connection((HOST, PORT), 10)
    s.sendall(hello)
    s.settimeout(5)

    def read_rec():
        hdr = s.recv(5)
        if len(hdr) < 5:
            return None
        ln = struct.unpack(">H", hdr[3:5])[0]
        body = s.recv(ln)
        return hdr, body

    sh = read_rec()
    print("client_hello_len", len(hello))
    print("server_hello", sh[0].hex() if sh else None, "body_len", len(sh[1]) if sh else 0)
    for i in range(4):
        rec = read_rec()
        if not rec:
            break
        hdr, body = rec
        print(f"rec{i}", hdr[0], hdr[1:3].hex(), len(body), body[:16].hex())
    s.close()


if __name__ == "__main__":
    main()
