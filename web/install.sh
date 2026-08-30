#!/usr/bin/env bash
# ==============================================================================
#  VITL Piano Desktop — Linux Native Installer
#  Usage: curl -fsSL https://raw.githubusercontent.com/RandomGuy-VN/VITL-PIANO-DESKTOP/main/web/install.sh | bash
# ==============================================================================
set -e

BOLD='\033[1m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

echo -e "${CYAN}${BOLD}"
echo "  ╔══════════════════════════════════════════════════════════╗"
echo "  ║         VITL Piano Desktop — Linux Installer             ║"
echo "  ║   High-Performance Virtual Piano Autoplayer & Synth      ║"
echo "  ╚══════════════════════════════════════════════════════════╝"
echo -e "${NC}"

# 1. OS & Architecture Check
OS="$(uname -s)"
ARCH="$(uname -m)"

if [ "$OS" != "Linux" ]; then
    echo -e "${RED}Error: This installer is for Linux only (detected: $OS).${NC}"
    exit 1
fi

if [ "$ARCH" != "x86_64" ]; then
    echo -e "${YELLOW}Warning: Detected architecture $ARCH. Official binaries are built for x86_64.${NC}"
fi

# 2. Installation Paths
INSTALL_DIR="$HOME/.local/share/vitl-piano"
BIN_DIR="$HOME/.local/bin"
DESKTOP_DIR="$HOME/.local/share/applications"
ICON_DIR="$HOME/.local/share/icons/hicolor/scalable/apps"

mkdir -p "$INSTALL_DIR" "$BIN_DIR" "$DESKTOP_DIR" "$ICON_DIR"

# 3. Extraction Helper
extract_package() {
    local zip_file="$1"
    local dest="$2"
    if command -v unzip >/dev/null 2>&1; then
        unzip -q -o "$zip_file" -d "$dest/"
    elif command -v python3 >/dev/null 2>&1; then
        python3 -c "import zipfile; zipfile.ZipFile('$zip_file').extractall('$dest')"
    elif command -v bsdtar >/dev/null 2>&1; then
        bsdtar -xf "$zip_file" -C "$dest/"
    elif command -v 7z >/dev/null 2>&1; then
        7z x "$zip_file" -o"$dest/" -y >/dev/null
    else
        echo -e "${RED}Error: Neither unzip, python3, bsdtar, nor 7z was found to extract the package.${NC}"
        exit 1
    fi
}

# 4. Determine Source (Local or Remote Download)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" 2>/dev/null && pwd || echo "")"
CWD="$(pwd)"
TMP_DIR="$(mktemp -d)"
cleanup() {
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT

echo -e "${BOLD}[1/4]${NC} Preparing VITL Piano binaries..."

INSTALLED_LOCALLY=false

# Check local file locations
if [ -n "$SCRIPT_DIR" ] && [ -f "$SCRIPT_DIR/vitl-piano-desktop" ]; then
    echo "Installing from: $SCRIPT_DIR"
    cp -f "$SCRIPT_DIR/vitl-piano-desktop" "$INSTALL_DIR/"
    cp -f "$SCRIPT_DIR/vitl-piano.sh" "$INSTALL_DIR/" 2>/dev/null || true
    cp -f "$SCRIPT_DIR/desktop.html" "$INSTALL_DIR/" 2>/dev/null || true
    cp -f "$SCRIPT_DIR/vitl-brand-logo.svg" "$INSTALL_DIR/" 2>/dev/null || true
    cp -f "$SCRIPT_DIR/vitl-brand-logo.png" "$INSTALL_DIR/" 2>/dev/null || true
    [ -d "$SCRIPT_DIR/lib" ] && cp -rf "$SCRIPT_DIR/lib" "$INSTALL_DIR/" 2>/dev/null || true
    INSTALLED_LOCALLY=true
elif [ -f "$CWD/vitl-piano-desktop" ]; then
    echo "Installing from current directory: $CWD"
    cp -f "$CWD/vitl-piano-desktop" "$INSTALL_DIR/"
    cp -f "$CWD/vitl-piano.sh" "$INSTALL_DIR/" 2>/dev/null || true
    cp -f "$CWD/desktop.html" "$INSTALL_DIR/" 2>/dev/null || true
    cp -f "$CWD/vitl-brand-logo.svg" "$INSTALL_DIR/" 2>/dev/null || true
    cp -f "$CWD/vitl-brand-logo.png" "$INSTALL_DIR/" 2>/dev/null || true
    [ -d "$CWD/lib" ] && cp -rf "$CWD/lib" "$INSTALL_DIR/" 2>/dev/null || true
    INSTALLED_LOCALLY=true
elif [ -n "$SCRIPT_DIR" ] && [ -f "$SCRIPT_DIR/../target/release/vitl-piano-desktop" ]; then
    echo "Installing from workspace: $SCRIPT_DIR/.."
    cp -f "$SCRIPT_DIR/../target/release/vitl-piano-desktop" "$INSTALL_DIR/"
    cp -f "$SCRIPT_DIR/../vitl-piano.sh" "$INSTALL_DIR/" 2>/dev/null || true
    cp -f "$SCRIPT_DIR/../desktop.html" "$INSTALL_DIR/" 2>/dev/null || true
    cp -f "$SCRIPT_DIR/../vitl-brand-logo.svg" "$INSTALL_DIR/" 2>/dev/null || true
    cp -f "$SCRIPT_DIR/../vitl-brand-logo.png" "$INSTALL_DIR/" 2>/dev/null || true
    [ -d "$SCRIPT_DIR/../lib" ] && cp -rf "$SCRIPT_DIR/../lib" "$INSTALL_DIR/" 2>/dev/null || true
    INSTALLED_LOCALLY=true
elif [ -f "$CWD/target/release/vitl-piano-desktop" ]; then
    echo "Installing from workspace: $CWD"
    cp -f "$CWD/target/release/vitl-piano-desktop" "$INSTALL_DIR/"
    cp -f "$CWD/vitl-piano.sh" "$INSTALL_DIR/" 2>/dev/null || true
    cp -f "$CWD/desktop.html" "$INSTALL_DIR/" 2>/dev/null || true
    cp -f "$CWD/vitl-brand-logo.svg" "$INSTALL_DIR/" 2>/dev/null || true
    cp -f "$CWD/vitl-brand-logo.png" "$INSTALL_DIR/" 2>/dev/null || true
    [ -d "$CWD/lib" ] && cp -rf "$CWD/lib" "$INSTALL_DIR/" 2>/dev/null || true
    INSTALLED_LOCALLY=true
elif [ -n "$SCRIPT_DIR" ] && [ -f "$SCRIPT_DIR/vitl-piano-linux.zip" ]; then
    echo "Extracting local package: $SCRIPT_DIR/vitl-piano-linux.zip"
    extract_package "$SCRIPT_DIR/vitl-piano-linux.zip" "$INSTALL_DIR"
    INSTALLED_LOCALLY=true
elif [ -f "$CWD/vitl-piano-linux.zip" ]; then
    echo "Extracting local package: $CWD/vitl-piano-linux.zip"
    extract_package "$CWD/vitl-piano-linux.zip" "$INSTALL_DIR"
    INSTALLED_LOCALLY=true
elif [ -f "$CWD/web/vitl-piano-linux.zip" ]; then
    echo "Extracting local package: $CWD/web/vitl-piano-linux.zip"
    extract_package "$CWD/web/vitl-piano-linux.zip" "$INSTALL_DIR"
    INSTALLED_LOCALLY=true
fi

# If not installed locally, download from remote mirrors
if [ "$INSTALLED_LOCALLY" != "true" ]; then
    MIRRORS=(
        "${VITL_BASE_URL:+$VITL_BASE_URL/vitl-piano-linux.zip}"
        "https://github.com/RandomGuy-VN/VITL-PIANO-DESKTOP/releases/latest/download/vitl-piano-linux.zip"
        "https://github.com/RandomGuy-VN/VITL-PIANO-DESKTOP/releases/download/v1.0.0/vitl-piano-linux.zip"
        "https://github.com/RandomGuy-VN/VITL-PIANO-DESKTOP/releases/download/v1.0-beta/vitl-piano-linux.zip"
    )

    DOWNLOADED=false
    for URL in "${MIRRORS[@]}"; do
        [ -z "$URL" ] && continue
        echo "Attempting download from: $URL"
        if command -v curl >/dev/null 2>&1; then
            if curl -fSL --connect-timeout 10 --progress-bar "$URL" -o "$TMP_DIR/package.zip" 2>/dev/null; then
                DOWNLOADED=true
                break
            fi
        elif command -v wget >/dev/null 2>&1; then
            if wget -q --timeout=10 --show-progress "$URL" -O "$TMP_DIR/package.zip" 2>/dev/null; then
                DOWNLOADED=true
                break
            fi
        fi
    done

    if [ "$DOWNLOADED" != "true" ] || [ ! -f "$TMP_DIR/package.zip" ]; then
        echo -e "${RED}Error: Failed to download release package. Please check your internet connection or download vitl-piano-linux.zip manually.${NC}"
        exit 1
    fi

    echo -e "${BOLD}[2/4]${NC} Extracting release files..."
    extract_package "$TMP_DIR/package.zip" "$INSTALL_DIR"
fi

chmod +x "$INSTALL_DIR/vitl-piano-desktop" "$INSTALL_DIR/vitl-piano.sh" 2>/dev/null || true

# Setup shared libraries compatibility
mkdir -p "$INSTALL_DIR/lib"
if [ ! -e "$INSTALL_DIR/lib/libjxl.so.0.12" ]; then
    JXL=$(ls /usr/lib/libjxl.so* /usr/lib64/libjxl.so* /usr/local/lib/libjxl.so* 2>/dev/null | head -n 1)
    [ -n "$JXL" ] && ln -sf "$JXL" "$INSTALL_DIR/lib/libjxl.so.0.12" 2>/dev/null || true
fi

# 5. Create Executable Symlink / Wrapper in ~/.local/bin
cat << 'WRAPPER' > "$BIN_DIR/vitl-piano"
#!/usr/bin/env bash
DIR="$HOME/.local/share/vitl-piano"
mkdir -p "$DIR/lib"
if [ ! -e "$DIR/lib/libjxl.so.0.12" ]; then
    JXL=$(ls /usr/lib/libjxl.so* /usr/lib64/libjxl.so* /usr/local/lib/libjxl.so* 2>/dev/null | head -n 1)
    [ -n "$JXL" ] && ln -sf "$JXL" "$DIR/lib/libjxl.so.0.12" 2>/dev/null || true
fi
export LD_LIBRARY_PATH="$DIR/lib:$DIR:$LD_LIBRARY_PATH"
rm -rf "$HOME/.cache/vitl-piano-desktop" "$HOME/.cache/vitl_piano"* 2>/dev/null || true
exec "$DIR/vitl-piano-desktop" "$@"
WRAPPER
chmod +x "$BIN_DIR/vitl-piano"

# 6. Setup Desktop Application Entry and Icons
echo -e "${BOLD}[3/4]${NC} Registering desktop application & icons..."
if [ -f "$INSTALL_DIR/vitl-brand-logo.svg" ]; then
    cp -f "$INSTALL_DIR/vitl-brand-logo.svg" "$ICON_DIR/vitl-piano.svg"
fi
if [ -f "$INSTALL_DIR/vitl-brand-logo.png" ]; then
    for size in 16 24 32 48 64 128 256 512; do
        mkdir -p "$HOME/.local/share/icons/hicolor/${size}x${size}/apps"
        if command -v rsvg-convert >/dev/null 2>&1 && [ -f "$INSTALL_DIR/vitl-brand-logo.svg" ]; then
            rsvg-convert -w "$size" -h "$size" "$INSTALL_DIR/vitl-brand-logo.svg" -o "$HOME/.local/share/icons/hicolor/${size}x${size}/apps/vitl-piano.png" 2>/dev/null || true
        else
            cp -f "$INSTALL_DIR/vitl-brand-logo.png" "$HOME/.local/share/icons/hicolor/${size}x${size}/apps/vitl-piano.png" 2>/dev/null || true
        fi
    done
fi

# Purge launcher icon caches
rm -rf "$HOME/.cache/thumbnails" "$HOME/.cache/icon-cache.kcache" "$HOME/.cache/fuzzel"* "$HOME/.cache/rofi"* 2>/dev/null || true
rm -f "$HOME/.local/share/icons/hicolor/icon-theme.cache" 2>/dev/null || true

cat << DESKTOP_ENTRY > "$DESKTOP_DIR/vitl-piano.desktop"
[Desktop Entry]
Name=VITL Piano
GenericName=Virtual Piano Autoplayer & Synthesizer
Comment=Hardware-level keystroke autoplayer and audio synthesizer for Roblox and Virtual Piano
Exec=$BIN_DIR/vitl-piano %U
Icon=vitl-piano
Terminal=false
Type=Application
Categories=AudioVideo;Audio;Game;Music;
Keywords=piano;midi;roblox;autoplayer;synthesizer;music;
StartupWMClass=vitl-piano-desktop
DESKTOP_ENTRY
chmod +x "$DESKTOP_DIR/vitl-piano.desktop"

if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$DESKTOP_DIR" 2>/dev/null || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" 2>/dev/null || true
fi

# 7. Check /dev/uinput permissions for Roblox Keystrokes
echo -e "${BOLD}[4/4]${NC} Checking hardware input permissions..."

if [ -w /dev/uinput ]; then
    echo -e "${GREEN}✓ /dev/uinput access: Ready (Full hardware keystroke emulation active)${NC}"
else
    echo -e "${YELLOW}Notice: Direct write access to /dev/uinput is recommended for Roblox Wayland macro.${NC}"
    if groups "$USER" 2>/dev/null | grep -q "\binput\b"; then
        echo -e "${GREEN}✓ User $USER is in the 'input' group.${NC}"
    else
        echo "To grant access without root, run:"
        echo -e "  ${CYAN}sudo usermod -aG input \$USER${NC}"
        echo "Then log out and log back in."
    fi
fi

# Check PATH
if [[ ":$PATH:" != *":$BIN_DIR:"* ]]; then
    echo -e "\n${YELLOW}Note: Add ~/.local/bin to your PATH to run 'vitl-piano' from any terminal:${NC}"
    echo -e "  ${CYAN}export PATH=\"\$HOME/.local/bin:\$PATH\"${NC} (in ~/.bashrc or ~/.zshrc)"
fi

echo -e "\n${GREEN}${BOLD}=================================================="
echo "   VITL Piano Desktop Installed Successfully!   "
echo -e "==================================================${NC}"
echo -e "\nTo launch VITL Piano:"
echo -e "  • Run ${CYAN}vitl-piano${NC} in your terminal"
echo -e "  • Or open ${CYAN}VITL Piano${NC} from your Application Launcher / Menu\n"
