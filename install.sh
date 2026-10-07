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

if [[ "${ARCH}" != "x86_64" && "${ARCH}" != "aarch64" && "${ARCH}" != "arm64" ]]; then
    echo -e "\033[1;31m✖ Error: Pre-built binaries are compiled for x86_64 and aarch64. Found ${ARCH}.\033[0m" >&2
    echo -e "  Please build from source: cargo install --git https://github.com/${REPO}.git" >&2
    exit 1
fi

if [[ "${ARCH}" == "aarch64" || "${ARCH}" == "arm64" ]]; then
    ARCH_SUFFIX="aarch64"
else
    ARCH_SUFFIX="x86_64"
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

    echo -e "  \033[1;34m📦\033[0m Syncing runtime assets (templates, static, wasm, assets)..."
    cp -r templates static wasm assets "${INSTALL_SHARE_DIR}/"
else
    TMP_DIR="$(mktemp -d)"
    trap 'rm -rf "${TMP_DIR}"' EXIT

    ASSET="deeperseeker-${ARCH_SUFFIX}-linux.tar.gz"

    # Resolve latest release version tag
    TARGET_VERSION=""
    if command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
        TARGET_VERSION="$(gh release view --repo "${REPO}" --json tagName -q .tagName 2>/dev/null || true)"
    fi
    if [[ -z "${TARGET_VERSION}" ]]; then
        TARGET_VERSION="$(curl -fsSL -H "Cache-Control: no-cache" -o /dev/null -w "%{url_effective}" "https://github.com/${REPO}/releases/latest" 2>/dev/null | awk -F'/' '{print $NF}' || true)"
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
        curl -fsSL -H "Cache-Control: no-cache" "${LATEST_URL}" -o "${TMP_DIR}/${ASSET}"
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
    if [[ -d "${TMP_DIR}/assets" ]]; then
        cp -r "${TMP_DIR}/assets" "${INSTALL_SHARE_DIR}/"
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

# 5. Interactive OpenCode Configuration
configure_opencode() {
    local endpoint_raw="${1:-}"
    local opencode_cfg_dir="${HOME}/.config/opencode"
    local opencode_cfg_file="${opencode_cfg_dir}/opencode.json"

    echo ""
    echo -e "  \033[1;34m⚙\033[0m Configuring OpenCode provider in ${opencode_cfg_file}..."
    mkdir -p "${opencode_cfg_dir}"

    if command -v python3 >/dev/null 2>&1; then
        python3 - "${opencode_cfg_file}" "${endpoint_raw}" << 'PYEOF'
import sys, json, os
from urllib.parse import urlparse

path = sys.argv[1]
raw = sys.argv[2].strip() if len(sys.argv) > 2 else ""

if not raw:
    base_url = "http://127.0.0.1:4000/v1"
else:
    if not (raw.startswith("http://") or raw.startswith("https://")):
        raw = "http://" + raw
    u = urlparse(raw)
    host = u.hostname or "127.0.0.1"
    if u.port:
        netloc = f"{host}:{u.port}"
    elif u.scheme == "http" and (host in ("127.0.0.1", "localhost") or "." not in host):
        netloc = f"{host}:4000"
    else:
        netloc = host
    path_part = u.path.rstrip("/")
    if not path_part.endswith("/v1") and path_part != "/v1":
        path_part = path_part + "/v1" if path_part else "/v1"
    base_url = f"{u.scheme}://{netloc}{path_part}"

data = {}
if os.path.exists(path):
    try:
        with open(path, "r", encoding="utf-8") as f:
            data = json.load(f)
    except Exception:
        data = {}

if "$schema" not in data:
    data["$schema"] = "https://opencode.ai/config.json"

if "provider" not in data or not isinstance(data["provider"], dict):
    data["provider"] = {}

data["provider"]["deeperseeker"] = {
    "npm": "@ai-sdk/openai-compatible",
    "name": f"DeeperSeeker ({base_url})",
    "options": {
        "baseURL": base_url,
        "apiKey": "dseeker"
    },
    "models": {
        "v4.1flash": {
            "name": "DeepSeek V4.1 Flash",
            "attachment": True,
            "modalities": {
                "input": ["text", "image"],
                "output": ["text"]
            }
        },
        "v4.1flash-think": {
            "name": "DeepSeek V4.1 Flash (Deep Think)",
            "attachment": True,
            "reasoning": True,
            "modalities": {
                "input": ["text", "image"],
                "output": ["text"]
            }
        },
        "v4.1flash-search": {
            "name": "DeepSeek V4.1 Flash (Web Search)",
            "attachment": True,
            "modalities": {
                "input": ["text", "image"],
                "output": ["text"]
            }
        },
        "v4.1flash-think-search": {
            "name": "DeepSeek V4.1 Flash (Think + Search)",
            "attachment": True,
            "reasoning": True,
            "modalities": {
                "input": ["text", "image"],
                "output": ["text"]
            }
        },
        "anthropic/claude-v4.1flash": {
            "name": "Claude V4.1 Flash",
            "attachment": True,
            "reasoning": True,
            "modalities": {
                "input": ["text", "image"],
                "output": ["text"]
            }
        }
    }
}

with open(path, "w", encoding="utf-8") as f:
    json.dump(data, f, indent=2)
    f.write("\n")

print(f"  \033[1;32m✔ Registered deeperseeker provider pointing to {base_url}\033[0m")
PYEOF
    else
        local base_url="http://127.0.0.1:4000/v1"
        if [[ -n "${endpoint_raw}" ]]; then
            base_url="${endpoint_raw}"
            [[ "${base_url}" != http://* && "${base_url}" != https://* ]] && base_url="http://${base_url}"
            [[ "${base_url}" != *:4000* && "${base_url}" != *:80* && "${base_url}" != *:443* ]] && base_url="${base_url}:4000"
            [[ "${base_url}" != */v1 ]] && base_url="${base_url}/v1"
        fi
        cat << JEOF > "${opencode_cfg_file}"
{
  "\$schema": "https://opencode.ai/config.json",
  "provider": {
    "deeperseeker": {
      "npm": "@ai-sdk/openai-compatible",
      "name": "DeeperSeeker (${base_url})",
      "options": {
        "baseURL": "${base_url}",
        "apiKey": "dseeker"
      },
      "models": {
        "v4.1flash": {
          "name": "DeepSeek V4.1 Flash",
          "attachment": true,
          "modalities": {
            "input": ["text", "image"],
            "output": ["text"]
          }
        },
        "v4.1flash-think": {
          "name": "DeepSeek V4.1 Flash (Deep Think)",
          "attachment": true,
          "reasoning": true,
          "modalities": {
            "input": ["text", "image"],
            "output": ["text"]
          }
        },
        "v4.1flash-search": {
          "name": "DeepSeek V4.1 Flash (Web Search)",
          "attachment": true,
          "modalities": {
            "input": ["text", "image"],
            "output": ["text"]
          }
        },
        "v4.1flash-think-search": {
          "name": "DeepSeek V4.1 Flash (Think + Search)",
          "attachment": true,
          "reasoning": true,
          "modalities": {
            "input": ["text", "image"],
            "output": ["text"]
          }
        },
        "anthropic/claude-v4.1flash": {
          "name": "Claude V4.1 Flash",
          "attachment": true,
          "reasoning": true,
          "modalities": {
            "input": ["text", "image"],
            "output": ["text"]
          }
        }
      }
    }
  }
}
JEOF
        echo -e "  \033[1;32m✔ Registered deeperseeker provider pointing to ${base_url}\033[0m"
    fi

    echo -e "    Test in OpenCode: \033[1mopencode --model deeperseeker/v4.1flash\033[0m"
}

PROMPT_ANSWER=""
if [[ -t 0 ]]; then
    echo -ne "\033[1;35m? Would you like to automatically configure OpenCode with DeeperSeeker? [y/N]: \033[0m"
    read -r PROMPT_ANSWER || true
elif [[ -t 1 && -r /dev/tty ]]; then
    echo -ne "\033[1;35m? Would you like to automatically configure OpenCode with DeeperSeeker? [y/N]: \033[0m" > /dev/tty
    read -r PROMPT_ANSWER < /dev/tty || true
fi

case "${PROMPT_ANSWER}" in
    [yY]|[yY][eE][sS])
        ENDPOINT_INPUT=""
        if [[ -t 0 ]]; then
            echo -ne "\033[1;35m? Enter DeeperSeeker host or baseURL [default: 127.0.0.1 (or 'mochi')]: \033[0m"
            read -r ENDPOINT_INPUT || true
        elif [[ -t 1 && -r /dev/tty ]]; then
            echo -ne "\033[1;35m? Enter DeeperSeeker host or baseURL [default: 127.0.0.1 (or 'mochi')]: \033[0m" > /dev/tty
            read -r ENDPOINT_INPUT < /dev/tty || true
        fi
        configure_opencode "${ENDPOINT_INPUT}"
        ;;
    *)
        echo -e "  \033[0;90mSkipped OpenCode configuration.\033[0m"
        ;;
esac

echo ""
echo -e "  \033[1mNeed help?\033[0m Run \033[1;36mdeeperseeker --help\033[0m"
echo ""
