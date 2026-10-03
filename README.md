<div align="center">

# ⚡ 深層探求者・DeeperSeeker-RS
### High-Performance DeepSeek Web Reverse Proxy in Rust

[![Rust](https://img.shields.io/badge/rust-2021_edition-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![Axum](https://img.shields.io/badge/axum-0.8-blue?style=flat-square&logo=tokio)](https://github.com/tokio-rs/axum)
[![License](https://img.shields.io/badge/license-MIT-green?style=flat-square)](LICENSE)
[![Status](https://img.shields.io/badge/status-active-success?style=flat-square)]()

*Ultra-low latency, memory-efficient reverse proxy bridging DeepSeek's Web API to OpenAI and Claude compatible endpoints.*

</div>

---

## 🏛️ Architecture

```
                   ┌────────────────────────────────────────┐
                   │    Client (OpenAI / Claude / Cursor)   │
                   └───────────────────┬────────────────────┘
                                       │ HTTP / SSE
                                       ▼
        ┌──────────────────────────────────────────────────────────────┐
        │                       deeperseeker-rs                        │
        │                                                              │
        │  ┌──────────────────┐  ┌────────────────┐  ┌──────────────┐  │
        │  │ OpenAI /v1/chat  │  │ Claude /v1/msg │  │  /dashboard  │  │
        │  └────────┬─────────┘  └───────┬────────┘  └──────┬───────┘  │
        │           │                    │                  │          │
        │           ▼                    ▼                  ▼          │
        │  ┌────────────────────────────────────────────────────────┐  │
        │  │  Scheduler v2 (Least In-Flight, Token Pool, Rate Limit)│  │
        │  └─────────────────────────────┬──────────────────────────┘  │
        │                                │                             │
        │  ┌─────────────────────────────┴──────────────────────────┐  │
        │  │  WASM PoW Engine (Embedded wasmtime Proof of Work)     │  │
        │  └─────────────────────────────┬──────────────────────────┘  │
        └────────────────────────────────┼─────────────────────────────┘
                                         │ Upstream Android API
                                         ▼
                             ┌───────────────────────┐
                             │   chat.deepseek.com   │
                             └───────────────────────┘
```

---

## ✨ Features

- ⚡ **Zero GIL, Blazing Fast**: Engineered with Axum 0.8 and Tokio for asynchronous high-concurrency throughput.
- 🧠 **DeepSeek V4.1 Flash**: Full streaming (SSE) and unary completions support with reasoning/thinking extraction.
- 🔁 **Scheduler v2**: Token pool management with round-robin, least-in-flight concurrency caps, and automatic cooldown recovery.
- 💬 **Conversation Persistence**: Multi-turn conversation signature tracking with upstream parent message pointer chaining.
- 🛡️ **Embedded WASM PoW Solver**: Native execution of DeepSeek's cryptographic Proof-of-Work solver via `wasmtime`.
- 📁 **Vision & File Uploads**: `/v1/files` and `/v1/files/upload` endpoints supporting documents and vision image recognition.
- 🎭 **Claude Messages API**: Drop-in compatibility for Claude Desktop via `/v1/messages`.
- 📊 **Web Dashboard**: Modern administration interface for token pool monitoring, real-time status, and token lifecycle management.

---

## 🚀 Quick Start

### 🪄 One-Liner (Pre-Built Linux x86_64 Binary)

No Rust toolchain required:

```bash
curl -fsSL https://github.com/Praveensenpai/deeperseeker-rs/releases/download/v0.1.0/deeperseeker-x86_64-linux.tar.gz | tar -xz
./deeperseeker
```

### 🛠️ Build from Source

```bash
# Clone the repository
git clone https://github.com/Praveensenpai/deeperseeker-rs.git
cd deeperseeker-rs

# Compile release binary
cargo build --release

# Run the proxy
./target/release/deeperseeker
```

The server listens on `http://127.0.0.1:4000`.

### 2. Configuration (.env)

| Variable | Default | Description |
| :--- | :--- | :--- |
| `DEEPSEEKER_PORT` | `4000` | Port to bind server |
| `DEEPSEEKER_HOST` | `0.0.0.0` | Bind host interface |
| `DEEPSEEKER_API_KEY` | `dseeker` | Bearer token for client requests |
| `DEEPSEEKER_ADMIN_USER` | `admin` | Admin dashboard username |
| `DEEPSEEKER_ADMIN_PASS` | `admin` | Admin dashboard password |
| `DEEPSEEKER_DB_PATH` | `deeperseeker.db` | SQLite database file path |
| `DEEPSEEKER_WASM_PATH` | `wasm/deepseek_pow_solver.wasm` | Path to PoW WebAssembly solver |

---

## 🔌 API Usage

### OpenAI Chat Completions

```bash
curl http://127.0.0.1:4000/v1/chat/completions \
  -H "Authorization: Bearer dseeker" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "v4.1flash",
    "messages": [{"role": "user", "content": "Hello!"}],
    "stream": true
  }'
```

### Claude Desktop Integration

Add to your `claude_desktop_config.json`:

```json
{
  "apiEndpoints": {
    "anthropic": {
      "baseUrl": "http://127.0.0.1:4000",
      "apiKey": "dseeker"
    }
  }
}
```

---

## 📊 Administration Dashboard

Navigate to `http://localhost:4000/dashboard`:
- **Username**: `admin`
- **Password**: `admin`

Add your DeepSeek account auth tokens to scale your token pool across multiple accounts.

---

## 📜 License

MIT License. Educational purposes only.
