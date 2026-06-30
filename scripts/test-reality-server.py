#!/usr/bin/env python3
import json
import paramiko
import time

HOST = "157.230.3.206"
USER = "root"
PASSWORD = "ZqF4THu3x9f"

cfg = {
    "log": {"level": "warn"},
    "inbounds": [{"type": "mixed", "listen": "127.0.0.1", "listen_port": 18080}],
    "outbounds": [
        {
            "type": "vless",
            "tag": "proxy",
            "server": "127.0.0.1",
            "server_port": 8447,
            "uuid": "550e8400-e29b-41d4-a716-446655440000",
            "tls": {
                "enabled": True,
                "server_name": "www.cloudflare.com",
                "utls": {"enabled": True, "fingerprint": "chrome"},
                "reality": {
                    "enabled": True,
                    "public_key": "VdvA1In4Po7ugbQHYIm518Vw5u72SFokjyTY4XwByRw",
                    "short_id": "a1b2c3d4",
                },
            },
        },
        {"type": "direct", "tag": "direct"},
    ],
    "route": {"final": "proxy"},
}

client = paramiko.SSHClient()
client.set_missing_host_key_policy(paramiko.AutoAddPolicy())
client.connect(HOST, username=USER, password=PASSWORD, timeout=30)
sftp = client.open_sftp()
with sftp.file("/tmp/reality-client.json", "w") as f:
    f.write(json.dumps(cfg, indent=2))
sftp.close()

for cmd in [
    "pkill -f '/tmp/reality-client.json' || true",
    "sing-box check -c /tmp/reality-client.json",
    "nohup sing-box run -c /tmp/reality-client.json >/tmp/reality-client.log 2>&1 &",
]:
    _, o, e = client.exec_command(cmd, timeout=30)
    o.channel.recv_exit_status()
    out = o.read().decode().strip()
    err = e.read().decode().strip()
    if out:
        print(out)
    if err:
        print("ERR:", err)

time.sleep(3)
_, o, e = client.exec_command(
    "curl -sS -o /dev/null -w 'HTTP:%{http_code}\\n' --connect-timeout 12 --max-time 15 "
    "-x http://127.0.0.1:18080 https://1.1.1.1/cdn-cgi/trace; "
    "tail -10 /tmp/reality-client.log; "
    "journalctl -u sing-box -n 3 --no-pager | grep reality || true",
    timeout=40,
)
print(o.read().decode())
print(e.read().decode())
client.close()
