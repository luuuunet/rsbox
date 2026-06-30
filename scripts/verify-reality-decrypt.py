#!/usr/bin/env python3
"""Verify REALITY session decrypt for a captured ClientHello record."""
import base64
import struct
import subprocess
import sys
from cryptography.hazmat.primitives.asymmetric import x25519
from cryptography.hazmat.primitives.ciphers.aead import AESGCM
from cryptography.hazmat.primitives.kdf.hkdf import HKDF
from cryptography.hazmat.primitives import hashes

PRIV_B64 = "SBmx3gF9whM-5Df2zEz2dBops2Dw6gIDakZ_-dRrDHw"


def hkdf_expand(ikm, salt, info=b"REALITY", length=32):
    return HKDF(hashes.SHA256(), length, salt, info).derive(ikm)


def find_key_share(body: bytes):
    sid_len = body[34]
    i = 35 + sid_len
    cs_len = struct.unpack(">H", body[i : i + 2])[0]
    i += 2 + cs_len
    comp_len = body[i]
    i += 1 + comp_len
    ext_len = struct.unpack(">H", body[i : i + 2])[0]
    i += 2
    end = i + ext_len
    while i + 4 <= end:
        et, el = struct.unpack(">HH", body[i : i + 4])
        ed = body[i + 4 : i + 4 + el]
        if et == 51:
            j = 2
            while j + 4 <= len(ed):
                group, klen = struct.unpack(">HH", ed[j : j + 4])
                key = ed[j + 4 : j + 4 + klen]
                if group == 0x001D and klen == 32:
                    return key
                j += 4 + klen
        i += 4 + el
    return None


def main():
    hello_b64 = sys.argv[1] if len(sys.argv) > 1 else None
    if not hello_b64:
        out = subprocess.check_output(
            ["cargo", "test", "-p", "rsb-protocol", "dump_reality_hello", "--", "--nocapture"],
            cwd=r"D:\morust\rsbox",
            stderr=subprocess.STDOUT,
            text=True,
        )
        for line in out.splitlines():
            if line.startswith("hello_b64="):
                hello_b64 = line.split("=", 1)[1]
                break
    record = base64.b64decode(hello_b64)
    raw = bytearray(record[5:])
    body = raw[4:]
    sid_len = body[34]
    sid_off = 39
    ciphertext = bytes(raw[sid_off : sid_off + sid_len])
    random = bytes(body[2:34])
    peer_pub = bytes(find_key_share(body))
    print("record_len", len(record), "sid_len", sid_len, "peer_pub", peer_pub[:8].hex() if peer_pub else None)

    priv = base64.urlsafe_b64decode(PRIV_B64 + "==")
    shared = x25519.X25519PrivateKey.from_private_bytes(priv).exchange(
        x25519.X25519PublicKey.from_public_bytes(peer_pub)
    )
    auth_key = hkdf_expand(shared, bytes(random[:20]))
    aad_zero = bytearray(raw)
    aad_zero[sid_off : sid_off + sid_len] = b"\x00" * sid_len
    for name, aad in [("zero", bytes(aad_zero)), ("original", bytes(raw))]:
        try:
            pt = AESGCM(auth_key).decrypt(random[20:32], ciphertext, aad)
            print(f"{name}_decrypt_ok", pt.hex())
        except Exception as e:
            print(f"{name}_decrypt_fail", e)


if __name__ == "__main__":
    main()
