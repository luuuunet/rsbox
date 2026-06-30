#!/usr/bin/env python3
import base64
import json
import paramiko
import subprocess

HOST = "157.230.3.206"
USER = "root"
PASSWORD = "ZqF4THu3x9f"

out = subprocess.check_output(
    ["cargo", "test", "-p", "rsb-protocol", "dump_reality_hello", "--", "--nocapture"],
    cwd=r"D:\morust\rsbox",
    stderr=subprocess.STDOUT,
    text=True,
)
hello_b64 = None
for line in out.splitlines():
    if line.startswith("hello_b64="):
        hello_b64 = line.split("=", 1)[1]
        break

remote = f'''
import base64, socket, struct
from cryptography.hazmat.primitives.asymmetric import x25519
from cryptography.hazmat.primitives.ciphers.aead import AESGCM
from cryptography.hazmat.primitives.kdf.hkdf import HKDF
from cryptography.hazmat.primitives import hashes

hello = base64.b64decode("{hello_b64}")
raw = bytearray(hello[5:])
body = raw[4:]
sid_len = body[34]
sid_off = 39
ct = bytes(raw[sid_off:sid_off+sid_len])
random = bytes(body[2:34])
# parse key share
sid_len2 = body[34]
i = 35 + sid_len2
cs_len = int.from_bytes(body[i:i+2], 'big'); i += 2 + cs_len
comp_len = body[i]; i += 1 + comp_len
ext_len = int.from_bytes(body[i:i+2], 'big'); i += 2
end = i + ext_len
peer = None
while i + 4 <= end:
    et, el = int.from_bytes(body[i:i+2],'big'), int.from_bytes(body[i+2:i+4],'big')
    ed = body[i+4:i+4+el]
    if et == 51:
        j = 2
        while j + 4 <= len(ed):
            g, kl = int.from_bytes(ed[j:j+2],'big'), int.from_bytes(ed[j+2:j+4],'big')
            key = bytes(ed[j+4:j+4+kl])
            if g == 0x001d and kl == 32:
                peer = key
            j += 4 + kl
    i += 4 + el
priv = base64.urlsafe_b64decode("SBmx3gF9whM-5Df2zEz2dBops2Dw6gIDakZ_-dRrDHw==")
shared = x25519.X25519PrivateKey.from_private_bytes(priv).exchange(x25519.X25519PublicKey.from_public_bytes(peer))
auth = HKDF(hashes.SHA256(), 32, random[:20], b"REALITY").derive(shared)
aad = bytearray(raw); aad[sid_off:sid_off+sid_len] = b"\\x00"*sid_len
try:
    pt = AESGCM(auth).decrypt(random[20:32], ct, bytes(aad))
    print("decrypt_ok", pt.hex())
except Exception as e:
    print("decrypt_fail", e)
s = socket.create_connection(("127.0.0.1", 8447), 5)
s.sendall(hello)
s.settimeout(3)
resp = s.recv(256)
s.close()
print("resp_len", len(resp), "type", resp[0] if resp else None)
'''

c = paramiko.SSHClient()
c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
c.connect(HOST, username=USER, password=PASSWORD, timeout=30)
_, o, e = c.exec_command(f"python3 - <<'PY'\n{remote}\nPY", timeout=40)
print(o.read().decode())
err = e.read().decode()
if err.strip():
    print('ERR', err[:500])
c.close()
