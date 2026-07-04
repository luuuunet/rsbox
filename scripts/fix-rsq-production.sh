#!/usr/bin/env bash
# Persist RSQ production fixes: disable obfs, keep conn_limit semantics on server binary.
set -euo pipefail
python3 - <<'PY'
import json
from pathlib import Path

p = Path("/etc/rsbox/rsqb-server.json")
cfg = json.loads(p.read_text())
for ib in cfg.get("inbounds", []):
    if ib.get("type") == "rsq":
        ib["obfs"] = {"enabled": False}
with open(p, "w") as f:
    json.dump(cfg, f, indent=2)

sp = Path("/etc/rsbox/sync-users.sh")
text = sp.read_text()
old_conn = '    if u.get("device_limit"):\n        row["conn_limit"] = int(u["device_limit"])'
new_conn = '''    dl = int(u.get("device_limit") or 0)
    if dl > 0:
        row["conn_limit"] = max(dl * 4, 8)'''
if old_conn in text:
    text = text.replace(old_conn, new_conn)
    sp.write_text(text)
patch = '''for ib in cfg.get("inbounds") or []:
    if ib.get("type") == "rsq":
        ib["obfs"] = {"enabled": False}
'''
if 'ib["obfs"] = {"enabled": False}' not in text:
    text = text.replace(
        'with open(cfg_path, "w") as f:',
        patch + 'with open(cfg_path, "w") as f:',
    )
    sp.write_text(text)
PY
systemctl restart rsbox-rsqb
sleep 2
systemctl is-active rsbox-rsqb
python3 -c "import json;d=json.load(open('/etc/rsbox/rsqb-server.json'));print('obfs=',d['inbounds'][0].get('obfs'))"
