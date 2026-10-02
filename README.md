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
│   ├── maukode-core/               # Session state, ContextManager (AGENTS.md), Drain loop, SQLite Store
│   ├── maukode-tui/                # Interactive Terminal UI (Ratatui + Crossterm)
│   └── maukode-cli/                # Binary CLI utama (`maukode`)
```

---

## 🚀 Fitur yang Sudah Diimplementasikan

- [x] **Interactive TUI Penuh (`ratatui` + `crossterm`)**:
  - Live token streaming, thinking/reasoning blocks, tool execution blocks, scrollable chat viewport.
  - Shortcut navigasi: `PageUp`/`PageDown` scroll, `Esc` cancel/dismiss, `Ctrl+C` quit.
- [x] **Slash Commands Interaktif di TUI**:
  - `/provider`: Buka modal pemilihan provider atau tekan `a` untuk menambahkan provider OpenAI-compatible baru (nama, base URL, API key, model default).
  - `/model`: Buka modal pemilih model yang secara otomatis mengambil daftar model dari endpoint `/v1/models` atau `/api/tags` provider.
  - `/new`: Memulai sesi baru yang bersih.
  - `/sessions`: Membuka modal riwayat sesi yang tersimpan untuk dimuat kembali.
  - `/clear`: Membersihkan tampilan pesan aktif.
  - `/help`: Menampilkan panduan bantuan perintah.
- [x] **Persistensi Sesi SQLite (`rusqlite`)**:
  - Semua pesan, tool calls, tool results, dan metadata tersimpan secara otomatis di `~/.local/share/maukode/maukode.db` (atau `~/.maukode/maukode.db`).
  - Lanjutkan sesi sebelumnya dengan `-c, --continue-session` atau `-s, --session <UUID>`.
- [x] **Konfigurasi Provider Persisten (`~/.config/maukode/config.json`)**:
  - Provider bawaan: Anthropic, OpenAI, Ollama (Local), OpenRouter.
  - Tambah provider OpenAI-compatible baru langsung dari TUI atau CLI.
- [x] **Universal Tool Calling & Settlement Loop**: Loop otomatis multi-turn yang mengeksekusi tool dan mengembalikan hasilnya ke LLM sampai turn selesai (*drain loop*).
- [x] **Built-in Coding Tools**:
  - `bash`: Eksekusi terminal dengan timeout dan output capture.
  - `read_file`: Membaca konten file dengan nomor baris (opsional range).
  - `write_file`: Menulis / membuat file baru beserta direktori parent.
  - `edit_file`: Penggantian substring teks secara bedah (*surgical edit*).
  - `glob`: Pencarian berkas berdasarkan pola.
  - `grep`: Pencarian isi file berbasis regex yang cepat.
- [x] **Context Discovery**: Menemukan dan menyertakan instruksi `AGENTS.md` dari root project secara otomatis ke dalam baseline system prompt.

---

## 🛠 Cara Membangun & Menjalankan

### Persyaratan
- Rust toolchain (Rust 1.80+ atau edisi 2021)

### Build
```bash
cargo build --release
```

### Menjalankan MauKode

```bash
# 1. Jalankan Interactive TUI (default)
./target/debug/maukode

# 2. Lanjutkan sesi sebelumnya di TUI
./target/debug/maukode --continue-session

# 3. Jalankan mode single-shot (headless command)
./target/debug/maukode run "buatkan file hello.rs yang mencetak halo dunia"

# 4. Mode CLI REPL sederhana (tanpa TUI)
./target/debug/maukode --cli
```

### Slash Commands di TUI

| Perintah | Deskripsi |
|---|---|
| `/provider` | Membuka popup provider. Tekan `a` untuk menambah provider OpenAI-compatible baru. |
| `/provider use <id>` | Langsung mengaktifkan provider tertentu (contoh: `/provider use ollama`). |
| `/provider add <name> <base_url> [key]` | Menambahkan provider OpenAI-compatible via teks perintah. |
| `/model` | Membuka popup pemilih model (mengambil daftar model dari API provider secara otomatis). |
| `/model <nama_model>` | Langsung mengganti model aktif (contoh: `/model gpt-4o-mini`). |
| `/sessions` | Membuka dialog untuk memuat riwayat sesi lama dari SQLite. |
| `/new` | Memulai percakapan baru. |
| `/clear` | Membersihkan layar chat saat ini. |
| `/help` | Menampilkan bantuan. |
| `/exit` | Keluar dari aplikasi. |
