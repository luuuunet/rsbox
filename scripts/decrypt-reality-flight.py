#!/usr/bin/env python3
"""Decrypt REALITY server encrypted flight (TLS 1.3) for sing-box vs rsbox compare."""
import base64
import hashlib
import hmac as py_hmac
import socket
import struct
import subprocess
import sys

from cryptography.hazmat.primitives.ciphers.aead import AESGCM
from cryptography.hazmat.primitives.asymmetric import x25519
from cryptography.hazmat.primitives.kdf.hkdf import HKDF, HKDFExpand
from cryptography.hazmat.primitives import hashes

HOST = sys.argv[1] if len(sys.argv) > 1 else "127.0.0.1"
PORT = int(sys.argv[2]) if len(sys.argv) > 2 else 8447
TAG = sys.argv[3] if len(sys.argv) > 3 else "server"


def cargo_hello_secret():
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
        body += sock.recv(ln - len(body))
    return hdr, body


def parse_server_pub(sh_body):
    i = 4 + 2 + 32
    sid_len = sh_body[i]
    i += 1 + sid_len + 2 + 1 + 2
    ext_len = struct.unpack(">H", sh_body[i : i + 2])[0]
    i += 2
    exts = sh_body[i : i + ext_len]
    j = 0
    while j + 4 <= len(exts):
        et, el = struct.unpack(">HH", exts[j : j + 4])
        j += 4
        data = exts[j : j + el]
        j += el
        if et == 0x33 and len(data) >= 4:
            klen = struct.unpack(">H", data[2:4])[0]
            if klen == 32:
                return data[4:36]
    return None


def hkdf_expand(prk, info, length):
    return HKDFExpand(hashes.SHA256(), prk, info, length).derive(b"")


def hkdf_extract(salt, ikm):
    return HKDF(hashes.SHA256(), 32, salt, b"").derive(ikm) if salt else py_hmac.new(b"\x00" * 32, ikm, hashlib.sha256).digest()


def derive_hs_keys(shared, transcript_hash):
    empty_hash = hashlib.sha256(b"").digest()
    zero = b"\x00" * 32
    early = hkdf_extract(empty_hash, zero)
    derived = hkdf_expand(early, b"tls13 derived" + empty_hash, 32)
    hs_secret = hkdf_extract(derived, shared)
    read_key = hkdf_expand(hs_secret, b"tls13 s hs traffic" + transcript_hash, 16)
    read_iv = hkdf_expand(hs_secret, b"tls13 s hs iv" + transcript_hash, 12)
    return read_key, read_iv


def record_nonce(base_iv, seq):
    nonce = bytearray(base_iv)
    seq_bytes = seq.to_bytes(8, "big")
    for i in range(8):
        nonce[4 + i] ^= seq_bytes[i]
    return bytes(nonce)


def decrypt_record(key, iv, seq, hdr, enc):
    nonce = record_nonce(iv, seq)
    plain = AESGCM(key).decrypt(nonce, enc, hdr)
    return plain[:-1] if plain and plain[-1] in (0x16, 0x17) else plain


def parse_hs_msgs(data):
    msgs = []
    i = 0
    while i + 4 <= len(data):
        ty = data[i]
        ln = int.from_bytes(data[i + 1 : i + 4], "big")
        end = i + 4 + ln
        if end > len(data):
            break
        msgs.append((ty, ln, data[i + 4 : end][:16].hex()))
        i = end
    return msgs


def main():
    hello, secret = cargo_hello_secret()
    s = socket.create_connection((HOST, PORT), 10)
    s.sendall(hello)
    sh_hdr, sh_body = read_rec(s)
    assert sh_hdr[0] == 0x16 and sh_body[0] == 2
    ccs = read_rec(s)
    assert ccs[0][0] == 0x14

    server_pub = parse_server_pub(sh_body)
    priv = x25519.X25519PrivateKey.from_private_bytes(secret)
    shared = priv.exchange(x25519.X25519PublicKey.from_public_bytes(server_pub))
    transcript = hello[5:] + sh_hdr + sh_body
    th = hashlib.sha256(transcript).digest()
    key, iv = derive_hs_keys(shared, th)

    seq = 0
    all_plain = b""
    enc_records = []
    s.settimeout(2)
    try:
        while True:
            rec = read_rec(s)
            if not rec:
                break
            hdr, body = rec
            if hdr[0] != 0x17:
                print(f"  skip record type {hdr[0]:#x}")
                continue
            enc_records.append(len(body))
            plain = decrypt_record(key, iv, seq, hdr, body)
            seq += 1
            all_plain += plain
    except Exception as e:
        print(f"  read stop: {e}")

    print(f"{TAG} encrypted_records={enc_records} total_plain={len(all_plain)}")
    for ty, ln, preview in parse_hs_msgs(all_plain):
        print(f"  hs type={ty:#x} len={ln} preview={preview}")
    s.close()


if __name__ == "__main__":
    main()
