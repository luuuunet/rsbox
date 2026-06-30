#!/usr/bin/env python3
import json
import paramiko

c = paramiko.SSHClient()
c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
c.connect("157.230.3.206", username="root", password="ZqF4THu3x9f", timeout=30)
_, o, _ = c.exec_command("cat /etc/sing-box/config.json")
cfg = json.loads(o.read().decode())
for ib in cfg["inbounds"]:
    if ib.get("tag") == "reality-in":
        print(json.dumps(ib, indent=2))
c.close()
