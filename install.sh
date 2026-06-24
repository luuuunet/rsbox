#!/usr/bin/env bash
# rsbox installation script
# Usage: curl -fsSL https://raw.githubusercontent.com/yourusername/rsbox/main/install.sh | bash

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Configuration
REPO="yourusername/rsbox"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
BINARY_NAME="rsbox"

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

# Get latest release version
get_latest_version() {
    echo -e "${YELLOW}Fetching latest version...${NC}"
    VERSION=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/')

    if [ -z "$VERSION" ]; then
        echo -e "${RED}Failed to fetch latest version${NC}"
        exit 1
    fi

    echo -e "${GREEN}Latest version: $VERSION${NC}"
}

# Download binary
download_binary() {
    DOWNLOAD_URL="https://github.com/$REPO/releases/download/$VERSION/rsbox-$OS-$ARCH"

    if [ "$OS" = "windows" ]; then
        DOWNLOAD_URL="${DOWNLOAD_URL}.exe"
    fi

    echo -e "${YELLOW}Downloading from: $DOWNLOAD_URL${NC}"

    TMP_FILE="/tmp/$BINARY_NAME"

    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$DOWNLOAD_URL" -o "$TMP_FILE"
    elif command -v wget >/dev/null 2>&1; then
        wget -q "$DOWNLOAD_URL" -O "$TMP_FILE"
    else
        echo -e "${RED}Neither curl nor wget found. Please install one of them.${NC}"
        exit 1
    fi

    if [ ! -f "$TMP_FILE" ]; then
        echo -e "${RED}Download failed${NC}"
        exit 1
    fi

    chmod +x "$TMP_FILE"
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
    echo "  1. Create a config file: config.json"
    echo "  2. Run: rsbox run -c config.json"
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
