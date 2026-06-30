#!/usr/bin/env python3
import json
import paramiko
import time

HOST = "157.230.3.206"
USER = "root"
PASSWORD = "ZqF4THu3x9f"

c = paramiko.SSHClient()
c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
c.connect(HOST, username=USER, password=PASSWORD, timeout=30)
_, o, _ = c.exec_command("cat /etc/sing-box/config.json")
cfg = json.loads(o.read().decode())
cfg["log"] = {"level": "trace", "timestamp": True}
with c.open_sftp().file("/etc/sing-box/config.json", "w") as f:
    f.write(json.dumps(cfg, indent=2))
c.exec_command("systemctl restart sing-box")[1].channel.recv_exit_status()
time.sleep(2)

# run rust hello sender on server
remote = r"""
import base64, socket, subprocess, sys
out = subprocess.check_output([
    'curl', '-sS', 'https://raw.githubusercontent.com/example/example/main/x'
], stderr=subprocess.DEVNULL)
"""
# instead embed hello from local
import subprocess
out = subprocess.check_output(
    ["cargo", "test", "-p", "rsb-protocol", "dump_reality_hello", "--", "--nocapture"],
    cwd=r"D:\morust\rsbox",
    stderr=subprocess.STDOUT,
    text=True,
)
hello_b64 = next(l.split("=", 1)[1] for l in out.splitlines() if l.startswith("hello_b64="))

script = f"""
import base64, socket
hello = base64.b64decode('{hello_b64}')
s = socket.create_connection(('127.0.0.1', 8447), 5)
s.sendall(hello)
s.settimeout(2)
try:
    print('resp', len(s.recv(256)))
except Exception as e:
    print('resp_err', e)
s.close()
"""
_, o, e = c.exec_command(f"python3 - <<'PY'\n{script}\nPY", timeout=20)
print(o.read().decode())
time.sleep(1)
_, o, _ = c.exec_command("journalctl -u sing-box -n 30 --no-pager | grep -i REALITY", timeout=20)
print(o.read().decode())
c.close()
