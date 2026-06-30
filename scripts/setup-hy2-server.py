#!/usr/bin/env python3
"""Install sing-box Hysteria2 server on test VPS."""
import paramiko
import textwrap
import secrets
import sys

HOST = "157.230.3.206"
USER = "root"
PASSWORD = "ZqF4THu3x9f"
DOMAIN = "s.lulunet.cc"
PORT = 3365
HY2_PASSWORD = "TestHy2_" + secrets.token_urlsafe(12)

CONFIG = textwrap.dedent(
    f"""
    {{
      "log": {{ "level": "info", "timestamp": true }},
      "inbounds": [
        {{
          "type": "hysteria2",
          "tag": "hy2-in",
          "listen": "::",
          "listen_port": {PORT},
          "users": [ {{ "password": "{HY2_PASSWORD}" }} ],
          "tls": {{
            "enabled": true,
            "server_name": "{DOMAIN}",
            "certificate_path": "/etc/sing-box/fullchain.pem",
            "key_path": "/etc/sing-box/privkey.pem"
          }}
        }}
      ],
      "outbounds": [ {{ "type": "direct", "tag": "direct" }} ],
      "route": {{ "final": "direct" }}
    }}
    """
)

SETUP = f"""set -e
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq curl openssl ca-certificates

VER=$(curl -fsSL https://api.github.com/repos/SagerNet/sing-box/releases/latest | grep tag_name | cut -d'"' -f4)
ARCH=$(uname -m)
case "$ARCH" in x86_64) SB_ARCH=amd64;; aarch64) SB_ARCH=arm64;; *) echo arch $ARCH; exit 1;; esac
curl -fsSL -o /tmp/sing-box.tgz "https://github.com/SagerNet/sing-box/releases/download/${{VER}}/sing-box-${{VER#v}}-linux-${{SB_ARCH}}.tar.gz"
tar -xzf /tmp/sing-box.tgz -C /tmp
install -m 755 /tmp/sing-box-*/sing-box /usr/local/bin/sing-box

mkdir -p /etc/sing-box
openssl req -x509 -nodes -newkey rsa:2048 -days 3650 \\
  -keyout /etc/sing-box/privkey.pem \\
  -out /etc/sing-box/fullchain.pem \\
  -subj "/CN={DOMAIN}"

cat > /etc/sing-box/config.json <<'EOFCONFIG'
{CONFIG}
EOFCONFIG

sing-box check -c /etc/sing-box/config.json

cat > /etc/systemd/system/sing-box.service <<'EOFSVC'
[Unit]
Description=sing-box
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=/usr/local/bin/sing-box run -c /etc/sing-box/config.json
Restart=on-failure
RestartSec=3
LimitNOFILE=1048576

[Install]
WantedBy=multi-user.target
EOFSVC

systemctl daemon-reload
systemctl enable sing-box
systemctl restart sing-box
sleep 2
systemctl is-active sing-box
ss -ulnp | grep {PORT} || ss -tlnp | grep {PORT}
echo HY2_PASSWORD={HY2_PASSWORD}
"""


def main():
    client = paramiko.SSHClient()
    client.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    client.connect(HOST, username=USER, password=PASSWORD, timeout=30)
    stdin, stdout, stderr = client.exec_command(SETUP, timeout=600)
    out = stdout.read().decode()
    err = stderr.read().decode()
    code = stdout.channel.recv_exit_status()
    print(out)
    if err:
        print(err, file=sys.stderr)
    client.close()
    if code != 0:
        sys.exit(code)
    print(f"\n# Client config snippet:")
    print(
        f'server={DOMAIN} port={PORT} password={HY2_PASSWORD} sni={DOMAIN} insecure=true'
    )


if __name__ == "__main__":
    main()
