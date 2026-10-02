# MauKode 🤖

**MauKode** adalah implementasi *open-source AI coding agent* berkinerja tinggi yang ditulis dalam bahasa **Rust** (diporting dan terinspirasi dari arsitektur [OpenCode](https://opencode.ai)).

MauKode dirancang untuk mengeksekusi instruksi coding secara otonom, bernalar menggunakan Large Language Models (LLM), mengeksekusi *tools* (seperti eksekusi terminal, baca/tulis/edit file, pencarian regex grep), dan terus beriterasi hingga tugas selesai.

---

## 🏗 Arsitektur Monorepo Cargo Workspace

Proyek ini dibangun sebagai Rust workspace modular yang memisahkan tanggung jawab secara ketat:

```text
maukode/
├── Cargo.toml                      # Workspace root manifest
├── crates/
│   ├── maukode-protocol/           # Definisi schema, Message, Role, ToolCall, ToolResult, LLMEvent
│   ├── maukode-llm/                # Adapter LLM streaming SSE (Anthropic & OpenAI/Ollama/OpenRouter)
│   ├── maukode-tools/              # Eksekutor tools: bash, read_file, write_file, edit_file, glob, grep
│   ├── maukode-core/               # Session state, ContextManager (AGENTS.md), Drain loop, Runner
│   └── maukode-cli/                # Binary CLI utama (maukode & maukode run)
```

---

## 🚀 Fitur yang Sudah Diimplementasikan

- [x] **Universal Tool Calling & Settlement Loop**: Loop otomatis multi-turn yang mengeksekusi tool dan mengembalikan hasilnya ke LLM sampai turn selesai (*drain loop*).
- [x] **Protokol Streaming LLM**:
  - **Anthropic Messages API**: Server-Sent Events (SSE) streaming dengan delta token, tool call start/delta/finish, dan usage.
  - **OpenAI Chat API & Compatible**: Mendukung OpenAI, Ollama, OpenRouter, Groq, dan model dengan reasoning delta (DeepSeek R1).
- [x] **Built-in Coding Tools**:
  - `bash`: Eksekusi terminal dengan timeout dan output capture.
  - `read_file`: Membaca konten file dengan nomor baris (opsional range).
  - `write_file`: Menulis / membuat file baru beserta direktori parent.
  - `edit_file`: Penggantian substring teks secara bedah (*surgical edit*).
  - `glob`: Pencarian berkas berdasarkan pola.
  - `grep`: Pencarian isi file berbasis regex yang cepat.
- [x] **Context Discovery**: Menemukan dan menyertakan instruksi `AGENTS.md` dari root project secara otomatis ke dalam baseline system prompt.
- [x] **Mode Eksekusi Ganda**:
  - Single-shot command: `maukode run "analisis bug ini"`
  - Interactive REPL: `maukode`

---

## 🛠 Cara Membangun & Menjalankan

### Persyaratan
- Rust toolchain (Rust 1.80+ atau edisi 2021)

### Build
```bash
cargo build --release
```

### Menjalankan MauKode

Pastikan API key telah diset di environment:

```bash
# Untuk Anthropic (Claude)
export ANTHROPIC_API_KEY="your-anthropic-key"
./target/debug/maukode run "buatkan fungsi fizzbuzz di Rust"

# Untuk OpenAI / Ollama / Local LLM
export OPENAI_API_KEY="your-openai-or-ollama-key"
./target/debug/maukode --provider openai --model gpt-4o run "refactor kode ini"

# Atau jalankan mode interaktif
./target/debug/maukode
```

---

## 🗺 Roadmap Selanjutnya

1. **Persistensi SQLite** (`sqlx`): Menyimpan histori sesi ke SQLite agar sesi dapat dilanjutkan (`--continue`).
2. **Interactive TUI**: Mengintegrasikan `ratatui` + `crossterm` untuk tampilan antarmuka terminal yang modern.
3. **PTY Support**: Menggunakan `portable-pty` untuk eksekusi proses background dan terminal interaktif.
4. **Daemon Server & ACP/MCP**: Menyediakan server HTTP/SSE (`axum`) untuk integrasi remote client dan plugin eksternal.
