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

c = paramiko.SSHClient()
c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
c.connect(HOST, username=USER, password=PASSWORD, timeout=30)
with c.open_sftp().file("/tmp/cap-client.json", "w") as f:
    f.write(json.dumps(cfg, indent=2))

cmd = """
pkill -f cap-client.json 2>/dev/null || true
pkill tcpdump 2>/dev/null || true
rm -f /tmp/sb.pcap
nohup sing-box run -c /tmp/cap-client.json >/tmp/cap-client.log 2>&1 &
sleep 2
(timeout 8 tcpdump -i lo -s 65535 -w /tmp/sb.pcap tcp port 8447 >/dev/null 2>&1 &) 
sleep 1
curl -sS -o /dev/null --connect-timeout 8 -x http://127.0.0.1:18080 https://1.1.1.1/cdn-cgi/trace || true
sleep 2
python3 - <<'PY'
import subprocess, struct
text = subprocess.check_output('tcpdump -r /tmp/sb.pcap -xx -c 1 2>/dev/null', shell=True).decode('latin1')
hexbytes = []
for line in text.splitlines():
    if '0x' in line and ':' in line:
        for p in line.split(':',1)[1].split():
            if len(p)==2:
                try: hexbytes.append(int(p,16))
                except: pass
# find TLS app data / handshake from client (first payload after TCP handshake)
data = bytes(hexbytes)
# crude: find 16 03 01 pattern
for i in range(len(data)-5):
    if data[i:i+3] == bytes([0x16,0x03,0x01]):
        ln = (data[i+3]<<8)|data[i+4]
        rec = data[i:i+5+ln]
        print('found_client_hello_len', len(rec))
        raw = rec[5:]
        print('sid_len', raw[38], 'sid_off39', raw[39:39+8].hex())
        break
else:
    print('no hello, pcap bytes', len(data))
PY
pkill -f cap-client.json 2>/dev/null || true
"""
_, o, e = c.exec_command(cmd, timeout=45)
print(o.read().decode())
if e.read().strip():
    print('ERR', e.read()[:300])
c.close()
