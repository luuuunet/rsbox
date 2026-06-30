#!/usr/bin/env python3
"""Send REALITY ClientHello to test server and inspect response."""
import base64
import os
import socket
import struct
import time

from cryptography.hazmat.primitives.asymmetric import x25519
from cryptography.hazmat.primitives.ciphers.aead import AESGCM
from cryptography.hazmat.primitives.kdf.hkdf import HKDF
from cryptography.hazmat.primitives import hashes
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat

HOST = "157.230.3.206"
PORT = 8447
PUBLIC_KEY_B64 = "VdvA1In4Po7ugbQHYIm518Vw5u72SFokjyTY4XwByRw"
SHORT_ID_HEX = "a1b2c3d4"
SNI = "www.cloudflare.com"


def hkdf_expand(ikm: bytes, salt: bytes, info: bytes, length: int = 32) -> bytes:
    return HKDF(
        algorithm=hashes.SHA256(),
        length=length,
        salt=salt,
        info=info,
    ).derive(ikm)


def build_minimal_client_hello(sni: str, pubkey: bytes, random: bytes, session_id: bytes) -> bytearray:
    exts = b""
    sni_bytes = sni.encode()
    sni_ext = struct.pack(">H", len(sni_bytes) + 3) + b"\x00" + struct.pack(">H", len(sni_bytes)) + sni_bytes
    exts += struct.pack(">HH", 0, len(sni_ext)) + sni_ext
    exts += struct.pack(">HHH", 43, 2, 0x0304)
    ks = struct.pack(">HH", 0x001D, 32) + pubkey
    ks_list = struct.pack(">H", len(ks)) + ks
    exts += struct.pack(">HH", 51, len(ks_list)) + ks_list
    exts += struct.pack(">HHH", 45, 1, 1)
    sig = struct.pack(">B", 8) + struct.pack(">8H", 0x0403, 0x0804, 0x0401, 0x0503, 0x0805, 0x0501, 0x0806, 0x0601)
    exts += struct.pack(">HH", 13, len(sig)) + sig

    body = b"\x03\x03" + random + bytes([32]) + session_id
    body += struct.pack(">H", 0x1301)
    body += b"\x01\x00"
    body += struct.pack(">H", len(exts)) + exts

    hs = b"\x01" + struct.pack(">I", len(body))[1:] + body
    return bytearray(b"\x16\x03\x01" + struct.pack(">H", len(hs)) + hs)


def main():
    priv = x25519.X25519PrivateKey.generate()
    pub = priv.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
    server_pub = base64.urlsafe_b64decode(PUBLIC_KEY_B64 + "==")
    shared = priv.exchange(x25519.X25519PublicKey.from_public_bytes(server_pub))

    random = os.urandom(32)
    session_plain = bytearray(32)
    session_plain[0:3] = b"\x00\x00\x00"
    session_plain[3] = 0
    struct.pack_into(">I", session_plain, 4, int(time.time()))
    sid = bytes.fromhex(SHORT_ID_HEX)
    session_plain[8 : 8 + len(sid)] = sid

    # Xray: SessionId field in ClientHello is zero when computing AAD.
    session_id_field = b"\x00" * 32
    hello = build_minimal_client_hello(SNI, pub, random, session_id_field)

    aad = bytes(hello[5:])
    auth_key = hkdf_expand(shared, random[:20], b"REALITY")
    sealed = AESGCM(auth_key).encrypt(random[20:32], bytes(session_plain[:16]), aad)
    off = 44
    hello[off : off + 32] = sealed

    s = socket.create_connection((HOST, PORT), 10)
    s.sendall(bytes(hello))
    s.settimeout(5)
    resp = s.recv(4096)
    s.close()
    print(f"sent {len(hello)} bytes, recv {len(resp)} bytes")
    if resp:
        print(f"record type {resp[0]} (22=0x16 handshake, 21=0x15 alert)")
        if resp[0] == 0x16 and len(resp) > 5:
            print(f"hs type {resp[5]} (2=ServerHello)")
        if resp[0] == 0x15:
            print(f"alert level={resp[5]} desc={resp[6]}")


if __name__ == "__main__":
    main()
