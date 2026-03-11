#!/bin/sh
# toll installer - https://github.com/Fullstop000/toll
# Usage: curl -fsSL https://raw.githubusercontent.com/Fullstop000/toll/refs/heads/master/install.sh | sh

set -eu

REPO="Fullstop000/toll"
BINARY_NAME="toll"
INSTALL_DIR="${TOLL_INSTALL_DIR:-$HOME/.local/bin}"

RED=$(printf '\033[0;31m')
GREEN=$(printf '\033[0;32m')
YELLOW=$(printf '\033[1;33m')
NC=$(printf '\033[0m')

info() {
    printf '%s[INFO]%s %s\n' "$GREEN" "$NC" "$1"
}

warn() {
    printf '%s[WARN]%s %s\n' "$YELLOW" "$NC" "$1"
}

error() {
    printf '%s[ERROR]%s %s\n' "$RED" "$NC" "$1" >&2
    exit 1
}

# Creates a temporary directory that works on both Linux and macOS.
make_temp_dir() {
    if TMPDIR_PATH=$(mktemp -d 2>/dev/null); then
        printf '%s\n' "$TMPDIR_PATH"
        return 0
    fi

    mktemp -d -t toll-installer
}

# Detects the supported operating system and normalizes it for release assets.
detect_os() {
    case "$(uname -s)" in
        Linux*) OS="linux" ;;
        Darwin*) OS="darwin" ;;
        *) error "Unsupported operating system: $(uname -s)" ;;
    esac
}

# Detects the supported CPU architecture and normalizes it for release assets.
detect_arch() {
    case "$(uname -m)" in
        x86_64|amd64) ARCH="x86_64" ;;
        arm64|aarch64) ARCH="aarch64" ;;
        *) error "Unsupported architecture: $(uname -m)" ;;
    esac
}

# Maps the detected platform to the expected GitHub release target triple.
get_target() {
    case "$OS" in
        linux)
            case "$ARCH" in
                x86_64) TARGET="x86_64-unknown-linux-musl" ;;
                aarch64) TARGET="aarch64-unknown-linux-musl" ;;
            esac
            ;;
        darwin)
            TARGET="${ARCH}-apple-darwin"
            ;;
    esac
}

# Fetches the latest GitHub release tag for the installer to download.
get_latest_version() {
    VERSION=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/' || true)
    if [ -z "${VERSION:-}" ]; then
        return 1
    fi
}

install_from_release() {
    DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${VERSION}/${BINARY_NAME}-${VERSION}-${TARGET}.tar.gz"
    TEMP_DIR=$(make_temp_dir)
    ARCHIVE="${TEMP_DIR}/${BINARY_NAME}.tar.gz"

    info "Downloading ${VERSION} for ${TARGET}"
    if ! curl -fsSL "$DOWNLOAD_URL" -o "$ARCHIVE"; then
        rm -rf "$TEMP_DIR"
        return 1
    fi

    info "Extracting release archive"
    if ! tar -xzf "$ARCHIVE" -C "$TEMP_DIR"; then
        rm -rf "$TEMP_DIR"
        return 1
    fi

    mkdir -p "$INSTALL_DIR"
    mv "${TEMP_DIR}/${BINARY_NAME}" "${INSTALL_DIR}/${BINARY_NAME}"
    chmod +x "${INSTALL_DIR}/${BINARY_NAME}"
    rm -rf "$TEMP_DIR"
    return 0
}

# Installs through cargo into a temporary root, then copies the final binary to INSTALL_DIR.
install_with_cargo() {
    if ! command -v cargo >/dev/null 2>&1; then
        return 1
    fi

    TEMP_ROOT=$(make_temp_dir)
    info "Falling back to cargo install"
    if ! cargo install "$BINARY_NAME" --locked --root "$TEMP_ROOT"; then
        rm -rf "$TEMP_ROOT"
        return 1
    fi

    mkdir -p "$INSTALL_DIR"
    cp "${TEMP_ROOT}/bin/${BINARY_NAME}" "${INSTALL_DIR}/${BINARY_NAME}"
    chmod +x "${INSTALL_DIR}/${BINARY_NAME}"
    rm -rf "$TEMP_ROOT"
    return 0
}

verify_install() {
    INSTALLED_PATH="${INSTALL_DIR}/${BINARY_NAME}"
    if [ ! -x "$INSTALLED_PATH" ]; then
        error "Installed binary is missing: ${INSTALLED_PATH}"
    fi

    info "Verification: $("$INSTALLED_PATH" --version 2>/dev/null || "$INSTALLED_PATH" -v 2>/dev/null || "$INSTALLED_PATH" --help | head -n 1)"

    case ":$PATH:" in
        *":${INSTALL_DIR}:"*) ;;
        *)
            warn "Installed to ${INSTALLED_PATH}, but ${INSTALL_DIR} is not in PATH"
            warn "Add this to your shell profile:"
            warn "  export PATH=\"${INSTALL_DIR}:\$PATH\""
            ;;
    esac
}

main() {
    info "Installing ${BINARY_NAME}"

    detect_os
    detect_arch
    get_target
    info "Detected platform: ${OS} ${ARCH} (${TARGET})"

    if get_latest_version && install_from_release; then
        info "Installed from GitHub Releases"
    else
        warn "Prebuilt binary unavailable for the latest release"
        if ! install_with_cargo; then
            error "Failed to install ${BINARY_NAME} from both GitHub Releases and cargo"
        fi
        info "Installed with cargo"
    fi

    verify_install
    printf '\n'
    info "Installation complete! Run '${BINARY_NAME} --help' to get started."
}

main
