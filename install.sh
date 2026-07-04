#!/usr/bin/env bash
# rsbox installation script
# Usage: curl -fsSL https://raw.githubusercontent.com/luuuunet/rsbox/main/install.sh | bash

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Configuration
REPO="luuuunet/rsbox"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
BINARY_NAME="rsbox"
MIN_RSQ_VERSION="0.1.5"

# Detect OS and architecture
detect_platform() {
    OS="$(uname -s)"
    ARCH="$(uname -m)"

    case "$OS" in
        Linux*)
            OS="linux"
            ;;
        Darwin*)
            OS="macos"
            ;;
        MINGW*|MSYS*|CYGWIN*)
            OS="windows"
            BINARY_NAME="rsbox.exe"
            ;;
        *)
            echo -e "${RED}Unsupported OS: $OS${NC}"
            exit 1
            ;;
    esac

    case "$ARCH" in
        x86_64|amd64)
            ARCH="x86_64"
            ;;
        aarch64|arm64)
            ARCH="aarch64"
            ;;
        *)
            echo -e "${RED}Unsupported architecture: $ARCH${NC}"
            exit 1
            ;;
    esac

    echo -e "${GREEN}Detected platform: $OS-$ARCH${NC}"
}

asset_name() {
    if [ "$OS" = "windows" ]; then
        echo "rsbox-windows-${ARCH}.exe"
    else
        echo "rsbox-${OS}-${ARCH}"
    fi
}

# Get release version (prefer latest tag >= MIN_RSQ_VERSION)
get_latest_version() {
    echo -e "${YELLOW}Fetching latest version...${NC}"
    VERSION=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/')

    if [ -z "$VERSION" ]; then
        VERSION="v${MIN_RSQ_VERSION}"
        echo -e "${YELLOW}Could not fetch latest release; falling back to ${VERSION}${NC}"
    else
        echo -e "${GREEN}Latest version: $VERSION${NC}"
    fi
}

rsbox_supports_rsq() {
    [ -x "$TMP_FILE" ] || return 1
    printf '%s' '{"inbounds":[{"type":"rsq","tag":"probe","listen":"127.0.0.1","listen_port":65503}]}' \
        | "$TMP_FILE" check -c /dev/stdin >/dev/null 2>&1
}

# Download binary
download_binary() {
    local asset
    asset="$(asset_name)"
    DOWNLOAD_URL="https://github.com/$REPO/releases/download/$VERSION/$asset"

    echo -e "${YELLOW}Downloading from: $DOWNLOAD_URL${NC}"

    TMP_FILE="/tmp/$BINARY_NAME"

    if command -v curl >/dev/null 2>&1; then
        if ! curl -fsSL "$DOWNLOAD_URL" -o "$TMP_FILE"; then
            echo -e "${RED}Download failed. RSQ support requires rsbox >= ${MIN_RSQ_VERSION}.${NC}"
            echo -e "${RED}See: https://github.com/$REPO/releases${NC}"
            exit 1
        fi
    elif command -v wget >/dev/null 2>&1; then
        if ! wget -q "$DOWNLOAD_URL" -O "$TMP_FILE"; then
            echo -e "${RED}Download failed. RSQ support requires rsbox >= ${MIN_RSQ_VERSION}.${NC}"
            exit 1
        fi
    else
        echo -e "${RED}Neither curl nor wget found. Please install one of them.${NC}"
        exit 1
    fi

    if [ ! -f "$TMP_FILE" ]; then
        echo -e "${RED}Download failed${NC}"
        exit 1
    fi

    chmod +x "$TMP_FILE"

    if ! rsbox_supports_rsq; then
        echo -e "${RED}Downloaded binary does not support RSQ inbound (need >= ${MIN_RSQ_VERSION}).${NC}"
        exit 1
    fi
}

# Install binary
install_binary() {
    echo -e "${YELLOW}Installing to $INSTALL_DIR/${NC}"

    mkdir -p "$INSTALL_DIR"
    mv "$TMP_FILE" "$INSTALL_DIR/$BINARY_NAME"

    echo -e "${GREEN}✓ Installed successfully!${NC}"
}

# Check if binary is in PATH
check_path() {
    if ! echo "$PATH" | grep -q "$INSTALL_DIR"; then
        echo ""
        echo -e "${YELLOW}Warning: $INSTALL_DIR is not in your PATH${NC}"
        echo "Add the following line to your shell profile (~/.bashrc, ~/.zshrc, etc.):"
        echo ""
        echo "    export PATH=\"\$PATH:$INSTALL_DIR\""
        echo ""
    fi
}

# Verify installation
verify_installation() {
    if [ -x "$INSTALL_DIR/$BINARY_NAME" ]; then
        echo ""
        echo -e "${GREEN}Installation verified!${NC}"
        echo ""
        "$INSTALL_DIR/$BINARY_NAME" version 2>/dev/null || echo "Version check skipped"
    else
        echo -e "${RED}Installation verification failed${NC}"
        exit 1
    fi
}

# Print usage instructions
print_usage() {
    echo ""
    echo "Next steps:"
    echo "  1. Generate QUIC TLS certs: rsbox rsq-gen-cert --output-dir ./certs --name your.domain"
    echo "  2. Create a config file: config.json (inbound type: rsq)"
    echo "  3. Run: rsbox run -c config.json"
    echo ""
    echo "For more information, visit:"
    echo "  https://github.com/$REPO"
    echo ""
}

# Main installation flow
main() {
    echo -e "${GREEN}rsbox Installation Script${NC}"
    echo ""

    detect_platform
    get_latest_version
    download_binary
    install_binary
    verify_installation
    check_path
    print_usage

    echo -e "${GREEN}Installation complete! 🎉${NC}"
}

main "$@"
