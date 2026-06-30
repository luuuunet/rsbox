#!/usr/bin/env python3
import base64
import paramiko
from cryptography.hazmat.primitives.asymmetric import x25519
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat, PrivateFormat, NoEncryption

HOST = "157.230.3.206"
USER = "root"
PASSWORD = "ZqF4THu3x9f"
PRIV_B64 = "SBmx3gF9whM-5Df2zEz2dBops2Dw6gIDakZ_-dRrDHw"
PUB_B64 = "VdvA1In4Po7ugbQHYIm518Vw5u72SFokjyTY4XwByRw"

# verify locally
pad = lambda s: s + "=" * (-len(s) % 4)
priv_bytes = base64.urlsafe_b64decode(pad(PRIV_B64))
pub_bytes = base64.urlsafe_b64decode(pad(PUB_B64))
priv = x25519.X25519PrivateKey.from_private_bytes(priv_bytes)
derived = priv.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
print("local derived pub:", base64.urlsafe_b64encode(derived).decode().rstrip("="))
print("expected pub:    ", PUB_B64)
print("match:", derived == pub_bytes)

client = paramiko.SSHClient()
client.set_missing_host_key_policy(paramiko.AutoAddPolicy())
client.connect(HOST, username=USER, password=PASSWORD, timeout=30)
_, o, _ = client.exec_command(
    "python3 - <<'PY'\n"
    "import base64\n"
    "from cryptography.hazmat.primitives.asymmetric import x25519\n"
    "from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat, PrivateFormat, NoEncryption\n"
    f"priv=base64.urlsafe_b64decode('{PRIV_B64}==')\n"
    f"pub=base64.urlsafe_b64decode('{PUB_B64}==')\n"
    "k=x25519.X25519PrivateKey.from_private_bytes(priv)\n"
    "d=k.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)\n"
    "print('server derived', base64.urlsafe_b64encode(d).decode().rstrip('='))\n"
    "print('match', d==pub)\n"
    "PY",
    timeout=30,
)
print(o.read().decode())
client.close()
