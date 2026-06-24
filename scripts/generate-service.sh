#!/usr/bin/env bash
# rsbox service generator script
# Generates systemd service files for Linux

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SERVICE_NAME="rsbox"
CONFIG_PATH="${CONFIG_PATH:-/etc/rsbox/config.json}"
BINARY_PATH="${BINARY_PATH:-/usr/local/bin/rsbox}"
USER="${SERVICE_USER:-root}"

usage() {
    cat <<EOF
Usage: $0 [OPTIONS]

Generate systemd service file for rsbox.

OPTIONS:
    -c, --config PATH     Config file path (default: /etc/rsbox/config.json)
    -b, --binary PATH     Binary path (default: /usr/local/bin/rsbox)
    -u, --user USER       Run as user (default: root)
    -h, --help            Show this help message

EXAMPLE:
    $0 -c /etc/rsbox/config.json -b /usr/local/bin/rsbox -u rsbox

    # Install the service:
    sudo cp rsbox.service /etc/systemd/system/
    sudo systemctl daemon-reload
    sudo systemctl enable rsbox
    sudo systemctl start rsbox

EOF
}

while [[ $# -gt 0 ]]; do
    case $1 in
        -c|--config)
            CONFIG_PATH="$2"
            shift 2
            ;;
        -b|--binary)
            BINARY_PATH="$2"
            shift 2
            ;;
        -u|--user)
            USER="$2"
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            usage
            exit 1
            ;;
    esac
done

OUTPUT_FILE="$SCRIPT_DIR/rsbox.service"

cat > "$OUTPUT_FILE" <<EOF
[Unit]
Description=rsbox - Rust sing-box compatible proxy
Documentation=https://github.com/yourusername/rsbox
After=network.target network-online.target
Wants=network-online.target

[Service]
Type=simple
User=$USER
Group=$USER
ExecStart=$BINARY_PATH run -c $CONFIG_PATH
ExecReload=/bin/kill -HUP \$MAINPID
Restart=on-failure
RestartSec=5s
LimitNOFILE=1048576
LimitNPROC=512

# Security
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/etc/rsbox /var/log/rsbox /var/lib/rsbox
PrivateTmp=true

# Capabilities (needed for TUN mode)
AmbientCapabilities=CAP_NET_ADMIN CAP_NET_BIND_SERVICE
CapabilityBoundingSet=CAP_NET_ADMIN CAP_NET_BIND_SERVICE

[Install]
WantedBy=multi-user.target
EOF

echo "✓ Generated: $OUTPUT_FILE"
echo ""
echo "Next steps:"
echo "  1. Review the service file: cat $OUTPUT_FILE"
echo "  2. Install: sudo cp $OUTPUT_FILE /etc/systemd/system/"
echo "  3. Reload: sudo systemctl daemon-reload"
echo "  4. Enable: sudo systemctl enable rsbox"
echo "  5. Start: sudo systemctl start rsbox"
echo "  6. Check status: sudo systemctl status rsbox"
echo ""
echo "View logs:"
echo "  sudo journalctl -u rsbox -f"
