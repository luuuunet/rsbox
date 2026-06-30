#!/usr/bin/env python3
import paramiko

c = paramiko.SSHClient()
c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
c.connect("157.230.3.206", username="root", password="ZqF4THu3x9f", timeout=30)
cmds = [
    "sing-box version",
    "curl -sS -o /dev/null -w 'ms:%{http_code}\\n' --connect-timeout 8 https://www.microsoft.com || true",
    "openssl s_client -connect www.microsoft.com:443 -servername www.microsoft.com </dev/null 2>/dev/null | head -5",
]
for cmd in cmds:
    print(">>>", cmd)
    _, o, e = c.exec_command(cmd, timeout=30)
    print(o.read().decode())
    err = e.read().decode().strip()
    if err:
        print("ERR:", err[:500])
c.close()
