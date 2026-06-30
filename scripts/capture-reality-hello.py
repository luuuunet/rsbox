#!/usr/bin/env python3
"""Capture REALITY ClientHello from sing-box client on server."""
import json
import paramiko

HOST = "157.230.3.206"
USER = "root"
PASSWORD = "ZqF4THu3x9f"

cfg = {
    "log": {"level": "warn"},
    "inbounds": [{"type": "mixed", "listen": "127.0.0.1", "listen_port": 18081}],
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
with sftp.file("/tmp/cap-client.json", "w") as f:
    f.write(json.dumps(cfg, indent=2))
sftp.close()

cmds = """
set -e
pkill -f cap-client.json 2>/dev/null || true
pkill tcpdump 2>/dev/null || true
rm -f /tmp/reality.pcap
timeout 12 tcpdump -i any -s 0 -w /tmp/reality.pcap 'host 127.0.0.1 and port 8447' >/tmp/tcpdump.log 2>&1 &
TP=$!
sleep 1
nohup sing-box run -c /tmp/cap-client.json >/tmp/cap-client.log 2>&1 &
sleep 2
curl -sS -o /dev/null --connect-timeout 8 -x http://127.0.0.1:18081 https://1.1.1.1/cdn-cgi/trace || true
sleep 2
pkill -f cap-client.json 2>/dev/null || true
wait $TP 2>/dev/null || true
echo PCAP:
tcpdump -r /tmp/reality.pcap -X -c 2 2>/dev/null | head -120
echo LOG:
tail -5 /tmp/cap-client.log
journalctl -u sing-box -n 3 --no-pager | grep reality || true
"""
_, o, e = client.exec_command(cmds, timeout=60)
print(o.read().decode())
err = e.read().decode()
if err.strip():
    print("ERR:", err[:1000])
client.close()
