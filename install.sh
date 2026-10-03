#!/usr/bin/env bash
# ==============================================================================
#  ⚡ 深索・DeeperSeeker-RS One-Liner Installer
# ==============================================================================

set -euo pipefail
IFS=$'\n\t'

REPO="Praveensenpai/deeperseeker-rs"
BINARY="deeperseeker"
INSTALL_BIN_DIR="${HOME}/.local/bin"
INSTALL_SHARE_DIR="${HOME}/.local/share/deeperseeker"

echo -e "\033[1;36m==>\033[0m \033[1mInstalling 深索・DeeperSeeker-RS (DeepSeek Web Reverse Proxy)...\033[0m"

# 1. Architecture & Platform Detection
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

if [[ "${OS}" != "linux" ]]; then
    echo -e "\033[1;31m✖ Error: Pre-built binaries are currently only supported on Linux.\033[0m" >&2
    exit 1
fi

if [[ "${ARCH}" != "x86_64" ]]; then
    echo -e "\033[1;31m✖ Error: Pre-built binaries are compiled for x86_64. Found ${ARCH}.\033[0m" >&2
    echo -e "  Please build from source: cargo install --git https://github.com/${REPO}.git" >&2
    exit 1
fi

mkdir -p "${INSTALL_BIN_DIR}"
mkdir -p "${INSTALL_SHARE_DIR}"

# 2. Local Repository Build or GitHub Release Download
if [[ -f "./Cargo.toml" ]] && grep -q 'name = "deeperseeker"' ./Cargo.toml; then
    LOCAL_VERSION="$(grep -E '^version\s*=' ./Cargo.toml | head -n1 | cut -d'"' -f2 || true)"
    if [[ -n "${LOCAL_VERSION}" ]]; then
        echo -e "  \033[1;34m⚡\033[0m Local repository checkout detected (\033[1;32mv${LOCAL_VERSION}\033[0m). Compiling release binary..."
    else
        echo -e "  \033[1;34m⚡\033[0m Local repository checkout detected. Compiling release binary..."
    fi
    cargo build --release
    install -m 755 "target/release/${BINARY}" "${INSTALL_BIN_DIR}/${BINARY}"

    echo -e "  \033[1;34m📦\033[0m Syncing runtime assets (templates, static, wasm)..."
    cp -r templates static wasm "${INSTALL_SHARE_DIR}/"
else
    TMP_DIR="$(mktemp -d)"
    trap 'rm -rf "${TMP_DIR}"' EXIT

    ASSET="deeperseeker-x86_64-linux.tar.gz"

    # Resolve latest release version tag
    TARGET_VERSION=""
    if command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
        TARGET_VERSION="$(gh release view --repo "${REPO}" --json tagName -q .tagName 2>/dev/null || true)"
    fi
    if [[ -z "${TARGET_VERSION}" ]]; then
        TARGET_VERSION="$(curl -fsSL -o /dev/null -w "%{url_effective}" "https://github.com/${REPO}/releases/latest" 2>/dev/null | awk -F'/' '{print $NF}' || true)"
    fi

    if [[ -n "${TARGET_VERSION}" ]]; then
        echo -e "  \033[1;34m↓\033[0m Downloading latest pre-built release (\033[1;32m${TARGET_VERSION}\033[0m)..."
    else
        echo -e "  \033[1;34m↓\033[0m Downloading latest pre-built release..."
    fi

    if command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
        gh release download --repo "${REPO}" --pattern "${ASSET}" --dir "${TMP_DIR}"
    else
        LATEST_URL="https://github.com/${REPO}/releases/latest/download/${ASSET}"
        curl -fsSL "${LATEST_URL}" -o "${TMP_DIR}/${ASSET}"
    fi

    echo -e "  \033[1;34m📦\033[0m Unpacking archive and deploying assets..."
    tar -xzf "${TMP_DIR}/${ASSET}" -C "${TMP_DIR}"

    install -m 755 "${TMP_DIR}/${BINARY}" "${INSTALL_BIN_DIR}/${BINARY}"

    if [[ -d "${TMP_DIR}/templates" ]]; then
        cp -r "${TMP_DIR}/templates" "${INSTALL_SHARE_DIR}/"
    fi
    if [[ -d "${TMP_DIR}/static" ]]; then
        cp -r "${TMP_DIR}/static" "${INSTALL_SHARE_DIR}/"
    fi
    if [[ -d "${TMP_DIR}/wasm" ]]; then
        cp -r "${TMP_DIR}/wasm" "${INSTALL_SHARE_DIR}/"
    fi
fi

# 3. Path Verification Notice
if [[ ":${PATH}:" != *":${INSTALL_BIN_DIR}:"* ]]; then
    echo ""
    echo -e "  \033[1;33mℹ Note: ${INSTALL_BIN_DIR} is not currently in your PATH.\033[0m"
    echo -e "  Add this line to your ~/.bashrc or ~/.zshrc:"
    echo -e "    \033[1mexport PATH=\"\${HOME}/.local/bin:\${PATH}\"\033[0m"
    echo ""
fi

# 4. Confirmation Banner & Getting Started Instructions
INSTALLED_VER=""
if [[ -x "${INSTALL_BIN_DIR}/${BINARY}" ]]; then
    INSTALLED_VER="$("${INSTALL_BIN_DIR}/${BINARY}" --version 2>/dev/null | awk '{print $2}' || true)"
fi

echo ""
if [[ -n "${INSTALLED_VER}" ]]; then
    echo -e "\033[1;32m✔ Successfully installed ${BINARY} (v${INSTALLED_VER}) to ${INSTALL_BIN_DIR}/${BINARY}\033[0m"
else
    echo -e "\033[1;32m✔ Successfully installed ${BINARY} to ${INSTALL_BIN_DIR}/${BINARY}\033[0m"
fi
echo -e "\033[0;90m  Assets deployed to: ${INSTALL_SHARE_DIR}\033[0m"
echo ""
echo -e "\033[1;36m🚀 What to run next:\033[0m"
echo ""
echo -e "  \033[1m1. Start the proxy server daemon:\033[0m"
echo -e "     \033[0;32mdeeperseeker serve\033[0m"
echo -e "     \033[0;90m(Listens on http://127.0.0.1:4000 with admin web dashboard at /dashboard)\033[0m"
echo ""
echo -e "  \033[1m2. Add your DeepSeek Web token to the pool:\033[0m"
echo -e "     \033[0;90mBrowser Console (chat.deepseek.com): JSON.parse(localStorage.getItem(\"userToken\")).value\033[0m"
echo -e "     \033[0;32mdeeperseeker token add \"<YOUR_TOKEN>\" --label \"primary\"\033[0m"
echo ""
echo -e "  \033[1m3. Inspect live telemetry & token consumption:\033[0m"
echo -e "     \033[0;32mdeeperseeker status\033[0m       \033[0;90m# Launch interactive Ratatui TUI dashboard\033[0m"
echo -e "     \033[0;32mdeeperseeker usage\033[0m        \033[0;90m# Input/Output token metrics (Today, Week, Month)\033[0m"
echo -e "     \033[0;32mdeeperseeker test\033[0m         \033[0;90m# Execute DB & WASM PoW diagnostics\033[0m"
echo ""
echo -e "  \033[1m4. Install as a background user service (optional):\033[0m"
echo -e "     \033[0;32mdeeperseeker service install\033[0m"
echo ""
echo -e "  \033[1mNeed help?\033[0m Run \033[1;36mdeeperseeker --help\033[0m"
echo ""
