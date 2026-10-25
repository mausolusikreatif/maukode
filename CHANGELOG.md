# Changelog

Semua perubahan penting untuk proyek ini akan didokumentasikan dalam file ini.

Format berdasarkan [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
dan proyek ini mengikuti [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.6.0] - 2026-10-25

### Fixed

- **Token cache terbaca di provider OpenAI-compatible**: usage kini membaca
  `prompt_tokens_details.cached_tokens` (OpenAI, GLM), `prompt_cache_hit_tokens` (DeepSeek), dan
  `cached_tokens` (Moonshot). Sebelumnya cache selalu tercatat 0. Bila provider tidak melaporkan,
  nilainya tetap "tidak diketahui", bukan 0.
- **Web UI aktif setelah `install.sh`**: daemon kini mencari web UI juga di
  `<prefix>/share/maukode/web` dan `~/.local/share/maukode/web`, sehingga `maukode server` langsung
  menyajikannya tanpa `--web-dir`. Pesan startup menampilkan folder yang dipakai, atau memberi tahu
  bila tidak ditemukan.
- **Versi binary mengikuti rilis**: versi workspace dinaikkan ke 0.5.0, dan workflow rilis gagal bila
  tag `v*` tidak sama dengan versi di `Cargo.toml`.

### Added

- **Hasil tool lama dipangkas tanpa menunggu jendela konteks penuh**: bila provider tidak punya
  prompt cache (tidak pernah melaporkan cache hit), hasil tool yang lebih lama dari jendela terbaru
  dipotong head/tail begitu timbunannya mencapai ±6k token, dan batas "lama" hanya maju per 6 pesan
  asisten supaya bagian yang sudah dipangkas tidak berubah di antaranya. Dengan cache, perilaku
  lama tetap berlaku (pangkas setelah konteks 60% penuh), karena menulis ulang pesan lama membuang
  cache. Sebelumnya, pada model berjendela 1 juta token pemangkasan praktis tidak pernah aktif.
- **Snapshot system prompt dan skema tool**: prompt dan skema tool yang dikirim ke model kini
  disimpan sekali per kombinasi unik (tabel `prompt_snapshots`, dideduplikasi dengan hash) dan
  dirujuk oleh setiap request lewat `usage_requests.snapshot_hash`. `GET
  /api/v1/sessions/{id}/system-prompt` menampilkan prompt, perkiraan token, dan ukuran tiap tool
  dari request terakhir sesi; snapshot yang tak lagi dirujuk ikut terhapus bersama sesinya.
- **Ukuran konteks vs total di pemakaian sesi**: web UI menampilkan "konteks 30k" (input request
  terakhir) di samping "total 546k" (jumlah semua request, karena riwayat dikirim ulang di tiap
  request), dan status bar TUI menampilkan `ctx`. Setiap request agen utama kini disimpan di tabel
  `usage_requests` (model, token masuk/keluar, cache), sehingga pertumbuhan token bisa diaudit.
  Request sub-agen hanya masuk ke total.
- **Output `bash` dikompres untuk `cargo` dan `git`**: pada perintah tunggal yang selesai dengan
  kode 0, baris progres (`Compiling`, `Downloading`, baris `test ... ok`, petunjuk `git status`,
  progres `git push`) dibuang sebelum masuk ke konteks, dan hasil `cargo` yang lebih dari 400 baris
  dipotong di tengah. Output yang gagal tidak pernah diubah, rantai/pipe/redirect tidak difilter,
  dan `MAUKODE_NO_COMPRESS=1` mematikannya.
- **`maukode update`**: memperbarui instalasi dari `install.sh` ke rilis terbaru GitHub
  (`--check` hanya melaporkan, `--version vX.Y.Z` memilih rilis). Tarball diverifikasi sha256-nya,
  web UI diganti lebih dulu lalu binary diganti dengan `rename`, dan daemon yang sedang jalan
  direstart. Binary hasil `cargo install` ditolak dengan petunjuk, begitu juga platform selain
  Linux x86_64.
- **Installer satu baris**: `curl -fsSL https://raw.githubusercontent.com/mausolusikreatif/maukode/master/install.sh | bash`
  mengunduh rilis terbaru (atau `--version vX.Y.Z`), memverifikasi sha256, lalu memasangnya ke
  `~/.local`. `--uninstall` menghapusnya.

### Performance

- **Prompt caching benar-benar terpakai**: urutan definisi tool kini tetap (diurutkan menurut nama),
  waktu saat ini tidak lagi ada di awal system prompt (cukup tanggal, di bagian akhir bersama memori
  yang di-recall), dan adapter Anthropic memasang `cache_control` di tool terakhir, system prompt
  yang stabil, dan pesan terakhir. Dalam satu turn, setiap langkah membaca request sebelumnya dari
  cache.
- **Kerja per prompt berkurang**: daemon menyimpan `ContextManager` (beserta cache parse repo map) dan
  memory store per workspace antar-turn, sehingga repo tidak di-parse ulang setiap prompt. Daftar
  tool MCP diambil sekali per koneksi.
- **`save_session` hanya menulis pesan yang berubah** (dibandingkan lewat hash isi), di thread
  blocking, dan `created_at` pesan lama tidak lagi di-reset. `messages_audit` kini diindeks dengan
  hash isi, bukan teks lengkap; database lama dimigrasikan otomatis saat dibuka.
- **Tool read-only berjalan paralel**: bila semua tool call dalam satu langkah hanya membaca
  (`read_file`, `grep`, `glob`, `web_search`, …), semuanya dijalankan bersamaan.
- **`grep` tidak lagi memblokir runtime**, menghormati `.gitignore`, melewati file biner dan file di
  atas 1 MB, dan menampilkan path relatif terhadap workspace.
- **`read_file` tanpa `end_line` mengembalikan paling banyak 2000 baris** (dan 2000 karakter per
  baris), dengan petunjuk cara membaca sisanya.
- **Cache prompt bertahan antar-turn**: konteks per-turn (tanggal dan memori yang di-recall) kini
  dikirim di akhir pesan user terakhir sebagai catatan `<maukode-context>`, bukan di system prompt.
  Riwayat percakapan sebelumnya tetap menjadi prefix yang ter-cache di Anthropic, provider
  OpenAI-compatible, dan paket ChatGPT.
- **Estimasi ukuran konteks lebih akurat**: pemeriksaan context window memakai ukuran yang
  dilaporkan provider untuk langkah sebelumnya, ditambah estimasi untuk pesan yang lebih baru saja;
  estimasi penuh kini ikut menghitung skema tool. `input_tokens` dari Anthropic kini sudah termasuk
  token yang ter-cache, sama seperti provider lain.
- **Compaction otomatis tidak diulang di setiap langkah**: setelah compaction (berhasil atau gagal),
  compaction berikutnya menunggu sampai percakapan bertambah 10% dari context window.
- **Delta streaming digabung**: daemon menggabungkan delta teks, reasoning, dan argumen tool yang
  berurutan (maksimal 50 ms) sebelum dipublikasikan, sehingga frame SSE jauh lebih sedikit dan log
  replay mencakup balasan yang jauh lebih panjang. TUI menggambar teks streaming pada tick 50 ms,
  bukan sekali per delta.
- **Regex HTML ke Markdown dikompilasi sekali** di `fetch_web_content`, bukan di setiap panggilan
  (dan per link atau item list).

### Fixed

- **Riwayat percakapan kini benar-benar append-only**:
  - Catatan konteks per-turn (tanggal dan memori) disimpan di pesan prompt-nya, tidak lagi
    dipindah ke pesan terakhir di setiap langkah.
  - System prompt dibekukan per sesi (dibangun ulang hanya setelah compaction).
  - Pemangkasan output tool lama baru dimulai saat konteks terisi 60%.
  - Akibatnya cache prompt tetap terpakai, dan blok thinking Claude tetap valid. Model baru menolak
    blok thinking yang riwayatnya diubah.
- **Compaction membuang blok thinking dari pesan yang dipertahankan**, karena blok itu terikat
  pada riwayat yang sudah diganti ringkasan.

- **Stream LLM yang macet tidak lagi menggantung turn**: semua client LLM punya connect timeout 30
  detik dan read timeout 300 detik; stream yang diam terlalu lama dicoba ulang.
- **Adapter Anthropic**: event `error` di tengah stream (misalnya `overloaded_error`) kini menjadi
  error (dan dicoba ulang bila sementara) alih-alih dianggap selesai; JSON argumen tool yang rusak
  menjadi error alih-alih `{}`; pemakaian token (termasuk cache read/write) dilaporkan lewat event
  `Usage`.
- **MCP**: daftar tool diambil ulang setelah server mengirim `notifications/tools/list_changed`, dan
  permintaan dari server (misalnya `ping`) kini dijawab alih-alih dianggap sebagai respons.

### Security

- **Whitelist bash tidak bisa lagi ditumpangi perintah lain**: entri `bash_whitelist` dulu dicocokkan
  sebagai awalan string, sehingga `cargo *` (ada di whitelist bawaan) juga meloloskan
  `cargo build && rm -rf ~` tanpa persetujuan, bahkan di mode `strict`. Sekarang entri hanya cocok
  dengan **satu perintah sederhana** (tanpa `;`, `&&`, `|`, redirection, `$(…)`, backtick, subshell,
  atau baris baru), dicocokkan kata per kata, hanya untuk tool `bash`, dan tidak pernah meloloskan
  perintah yang dinilai berbahaya.
- **Persetujuan "untuk sesi ini" kini per tool**: menyetujui `memory_get {"id":1}` tidak lagi ikut
  meloloskan `memory_delete {"id":1}`.
- **Penilaian risiko bash melihat isi perintah yang dibungkus**: `bash -c '…'`, `sh -c`, `eval`,
  subshell `( … )`, `<( … )`, `{ …; }`, `if … then`, brace expansion `{rm,-rf,x}`, here-string
  `bash <<< '…'`, wrapper (`timeout`, `nice`, `nohup`, `xargs`, `busybox`, …), `find -exec`, dan
  `git -C dir …` kini diperiksa isinya. Kode yang tidak bisa diperiksa (`python -c`, `node -e`,
  `perl -e`, shell/interpreter yang membaca dari stdin, nama perintah dari variabel, `awk` dengan
  `system()`) minimal menjadi peringatan sehingga mode `ask` meminta persetujuan. Begitu juga tulis
  ke luar workspace (`>> ~/.bashrc`, `tee /etc/…`, `cp`/`mv` ke path absolut), `git checkout -- .`,
  `git restore`, dan `git push --force-with-lease`/`--delete`. Membaca `daemon.json` atau
  `chatgpt_auth.json` dan memanggil API daemon lokal (`curl localhost:4096/api/v1/…`) dianggap
  berbahaya.
- **Subagen kini meminta persetujuan**: tool call subagen dulu berjalan tanpa permission manager
  apa pun. Sekarang subagen memakai mode izin dan whitelist sesi induknya, dan permintaan izinnya
  muncul di sesi induk.
- **Daemon lokal selalu memakai token**: `maukode` dan `maukode server start` membuat token acak bila
  tidak diberi, menyimpannya di `daemon.json` (hanya bisa dibaca pemilik), dan klien lokal
  memakainya otomatis. `maukode server start|status|restart` mencetak alamat web UI lengkap dengan
  `#token=…`. Membuka `http://127.0.0.1:4096` tanpa token kini meminta token.
- **`maukode server` menolak alamat non-loopback tanpa token** (misalnya `--bind 0.0.0.0:4096`);
  token kosong dianggap tidak ada token. Token dibandingkan dalam waktu konstan. Origin `tauri://`
  tidak lagi diterima.
- **Server MCP dari repo tidak otomatis jalan**: `mcp.json` / `.maukode/mcp.json` milik project
  hanya dipakai bila folder tersebut ada di `security.trusted_projects`. Tambahkan dengan
  `maukode trust` (cabut dengan `maukode trust --revoke`). Server MCP dari konfigurasi global tetap
  berjalan seperti biasa.
- **`fetch_web_content` menolak alamat metadata cloud dan API daemon**: alamat link-local
  (`169.254.169.254`, `fe80::/10`), alamat metadata lain, dan `localhost/api/…` ditolak, termasuk
  lewat redirect dan nama DNS. Body dibaca bertahap dan berhenti di batas 2 MB.
- **`config.json` kini hanya bisa dibaca pemiliknya** (0600) karena berisi API key.
- **Tool MCP kini meminta persetujuan di mode `ask`**: tool MCP menjalankan kode yang tidak bisa
  diperiksa MauKode, jadi setiap panggilannya kini meminta izin, kecuali tool yang ditandai server
  dengan `readOnlyHint`. "Izinkan untuk sesi ini" berlaku untuk tool tersebut dengan argumen apa
  pun.
- **`maukode trust` tidak lagi bisa dibatalkan diam-diam oleh daemon**: hanya `maukode trust` yang
  menulis `security.trusted_projects`. Penyimpanan lain (API pengaturan daemon, CLI) mempertahankan
  daftar yang ada di disk, sehingga daemon yang menyimpan konfigurasi lamanya dari memori tidak lagi
  menghapus project yang di-trust saat daemon berjalan.

### Added

- **Web UI: palet perintah, arsip sesi, dan buka di editor**:
  - `Ctrl/Cmd+K` membuka palet untuk berpindah sesi (cari judul), navigasi, dan aksi sesi
    (lihat perubahan, hentikan agent, arsipkan). `?` menampilkan daftar pintasan.
  - Arsip sesi kini disimpan di daemon (`PUT /sessions/{id}/archive`), sehingga sama di semua
    perangkat; arsip lama di browser dipindahkan otomatis. Sidebar punya tombol Arsip serta
    Arsipkan/Pulihkan.
  - "Buka di editor" di panel perubahan dan di tool call file membuka file dengan
    `editor.command` (default `code --goto {path}:{line}`) di mesin tempat daemon berjalan.
- **`maukode run --output json|stream-json`** untuk CI dan skrip. Mode `json` mencetak satu objek
  hasil (teks akhir, stop reason, tool call, izin yang ditolak, error, dan usage beserta biaya).
  Mode `stream-json` mencetak setiap event sebagai satu baris JSON, lalu objek hasilnya. Di kedua
  mode, panggilan yang butuh izin otomatis ditolak; `--permission-mode` mengubahnya untuk run
  tersebut. Exit code 1 bila turn gagal.
- **TUI: steering, antrean, dan autocomplete `@file`**:
  - `Enter` saat agen bekerja mengantrekan prompt, yang lalu dikirim berurutan setelah turn
    selesai. `/queue` menampilkan, menjalankan, atau mengosongkan antrean.
  - `Alt+Enter` atau `/steer` menyisipkan arahan ke turn yang sedang berjalan.
  - Mengetik `@` membuka daftar file workspace dengan pencocokan fuzzy yang menghormati
    `.gitignore`.
- **Tool `apply_patch`**: menambah, mengubah, menghapus, dan memindahkan beberapa file sekaligus
  dalam satu panggilan dengan format `*** Begin Patch`. Semua hunk diperiksa dulu, sehingga patch
  diterapkan seluruhnya atau tidak sama sekali.
- **Aturan izin berbasis pola** (`security.permission_rules`): izinkan, tanyakan, atau tolak tool
  call berdasarkan nama tool, pola perintah bash, atau glob path, di semua mode izin. `deny` selalu
  menang.
- **Hooks** (`hooks`): perintah shell untuk `pre_tool_use` (bisa memblokir), `post_tool_use`
  (misalnya formatter setelah edit; output kegagalan diteruskan ke model), dan `turn_end`. Hooks
  dari `.maukode/hooks.json` milik project hanya jalan untuk project yang di-trust.
- **Pemakaian token dan biaya per sesi**: total token (input, output, cache read/write) dan biaya
  setiap sesi disimpan, termasuk pemakaian subagen. Totalnya tampil di composer web UI dan status
  bar TUI. Harga bawaan tersedia untuk model Claude terkini; model lain bisa diberi harga lewat
  `models.<id>.pricing` di config. Daemon mengirim event `session_usage`, dan snapshot sesi kini
  punya field `usage`.
- **Extended thinking untuk Anthropic**: thinking adaptif aktif otomatis di Claude 4.6 ke atas,
  dengan budget tetap untuk model yang lebih lama. Effort dan tampilan ringkasan bisa diatur per
  model lewat `models.<id>.thinking`. Teks thinking tampil sebagai proses berpikir, dan blok
  thinking beserta signature-nya dikirim balik utuh di langkah berikutnya.
- **Input gambar**: gambar PNG, JPEG, GIF, atau WebP (maksimal 5 MB, 20 per pesan) bisa dilampirkan
  di web UI lewat tombol, tempel, atau seret, lalu dikirim ke Anthropic, provider
  OpenAI-compatible, dan paket ChatGPT. API: `images` di `POST /sessions/{id}/prompt`.

- **Tempat perintah dijalankan bisa dipilih (`ExecBackend`)**: tool `bash` tidak lagi selalu
  menjalankan proses lokal. Pengaturan baru `execution` memilih **Host** (seperti biasa) atau
  **Lawang**: sel terisolasi milik daemon lawang (program terpisah, dipakai
  lewat HTTP) dengan tingkat `process` (bubblewrap), `sandbox` (gVisor) atau `vm` (Firecracker).
  Konfigurasi lama tanpa bagian `execution` tetap berjalan di host.
  - Satu sel per sesi, dibuat saat perintah pertama dan dihapus bersama sesinya. Folder proyek
    dipasang ke sel di `/work`; untuk VM, folder disalin masuk dan dibawa pulang. Tool file
    (`read_file`, `grep`, `glob`, `write_file`, `edit_file`) tetap bekerja di folder host dan
    dibungkus `SyncedTool` supaya perubahan di kedua sisi saling terlihat. File yang hanya ada di
    host tidak pernah dihapus oleh sinkronisasi.
  - Subagen menjalankan perintah di tempat yang sama dengan sesi induknya.
  - Daemon tidak bisa dihubungi, token salah, atau konfigurasi rusak (misalnya VM tanpa image)
    membuat perintah gagal dengan penjelasan. **Perintah tidak pernah jatuh diam-diam ke host.**
  - API: `GET|PUT /api/v1/config/execution` dan `GET /api/v1/config/execution/status` (menanyakan
    daemon tingkat isolasi mana yang tersedia). Token tidak pernah dikirim balik.
  - UI: bagian **Eksekusi** di Pengaturan dan label (Host / Proses terisolasi / Sandbox / VM) di
    header sesi.
  - Batasan: server MCP tetap berjalan di host; CLI/TUI belum memakai backend ini; pengaturan
    berlaku untuk semua sesi sekaligus.

### Changed

- **Dokumentasi dirapikan agar sesuai kode**: README ditulis ulang (crate `maukode-memory`, web UI,
  memori, subagen, backend eksekusi lawang, daftar tool dan slash command lengkap, penjelasan
  keamanan yang jujur: path jail hanya untuk tool file, bukan sandbox untuk `bash`).
  - Status fitur kini hanya ada di `docs/ROADMAP.md`, termasuk backlog hasil audit kode
    (performa, keandalan, keamanan, fitur). `docs/ROADMAP_AUDIT.md` dihapus.
  - `docs/DESKTOP_ROADMAP.md` menjadi `docs/WEB_UI_ROADMAP.md`, tanpa sisa Tauri.
  - `docs/CONFIGURATION.md` mengikuti skema asli (`providers` berupa list dengan `kind`, blok
    `agent` dan `execution`; blok `compaction` yang tidak pernah ada dihapus).
  - `docs/ARCHITECTURE.md`, `docs/TUI_ROADMAP.md`, `docs/CI_TEST_PLAN.md`, dan
    `apps/desktop/README.md` diperbarui; skill rilis di `.maukode/skills/learned/` tidak lagi
    menyuruh mengubah file Tauri.
  - `AGENTS.md` mencatat aturan bahasa dokumentasi dan bahwa status fitur dicatat di
    `docs/ROADMAP.md`.

## [0.4.0] - 2026-10-15

Minor release. Default langkah agent 50 → 2000, perbaikan pengiriman CLI `--max-steps`, dan retry stream llm-kita.

### Fixed

- **CLI `--max-steps` benar-benar terkirim ke daemon**: `ServerClient::set_agent_config`
  POST ke path yang salah (`/api/v1/agent/config`, selalu 404) sementara pemanggil
  di CLI menelan error via `let _ =`, sehingga override selalu gagal diam-diam.
  Sekarang mengarah ke `/api/v1/config/agent` dan kegagalan diteruskan sebagai
  error eksplisit (`Failed to apply --max-steps {n}: ...`).
- **LLM stream body-cut llm-kita di-retry**: `LLMError::is_transient()` sekarang
  mengenali pesan reqwest/hyper seperti `error decoding response body: stream
  closed before response completed`, sehingga turn yang terputus di tengah jalan
  di-resume (maks 4 attempt) alih-alih langsung gagal.

### Changed

- **Default `max_steps` 50 → 2000** (`AgentConfig` dan `RunnerConfig::default`).
  Batas validasi server `POST /config/agent` naik dari `1..=200` ke `1..=10000`.
  Config tersimpan yang sudah ada (`config.json`) tetap dihormati dan tidak
  dimigrasi paksa; setel ulang sekali via `--max-steps`, Settings, atau edit file.

## [0.3.2] - 2026-10-14

Patch release. Menghapus OpenCode config sync yang mengoverride pilihan model user.

### Added

- **Memory Consolidation Tier 4 (Abstraction Synthesis)**: Setelah merge duplikat,
  engine consolidation sekarang dapat mensintesis memory abstraksi tingkat tinggi
  dari pola lintas memory dalam taxonomy branch yang sama. Abstraksi baru ditulis
  dengan `[[Title]]` link balik ke source memories. Dikendalikan oleh
  `ConsolidationConfig.abstraction` dan `abstraction_min_memories`.
- **UI Settings Consolidation**: Tambah toggle consolidation dan abstraction di
  Settings page desktop app.

### Removed

- **OpenCode config sync**: Menghapus `sync_from_opencode_if_present()` sepenuhnya.
  Fungsi ini sebelumnya membaca `~/.config/opencode/opencode.json` saat setiap startup
  dan mengoverride `active_model` serta provider `default_model` dengan nilai dari
  OpenCode (misal `"auto"`), lalu menyimpan ulang config yang sudah ter-overwrite
  ke disk. MauKode adalah project mandiri dan tidak seharusnya membaca config
  OpenCode. Env-var API key fallback tetap dipertahankan.

## [0.3.1] - 2026-10-14

Patch release. 1 PR (#72) sejak v0.3.0.

### Fixed

- **Sinkronisasi Session Model**: Memperbaiki issue di mana pemilihan model via
  TUI (`/model`, `ModelSelector` via F2, dan `ProviderSelector` via F3) atau saat
  pembuatan sesi baru tidak tersinkronisasi ke sesi yang tersimpan di SQLite database.
  Akibatnya server `effective_model` tetap terkunci pada model default sesi
  (misal `"auto"`). Sekarang memanggil `set_session_model` (`PUT /api/v1/sessions/{id}/model`)
  dan menyinkronkan status sesi secara konsisten.
- **Propagasi CLI `--max-steps`**: Memperbaiki flag CLI `maukode --max-steps <N>` yang
  sebelumnya terabaikan dan selalu fallback ke default 50 steps pada daemon mode
  dan standalone TUI mode. Sekarang nilai `--max-steps` dikirim ke endpoint
  `POST /api/v1/agent/config` (`client.set_agent_config`) pada daemon mode dan
  diteruskan ke `run_tui` pada standalone mode.
- **Context Window Gateway Routing (`"auto"`)**: Memperbaiki resolusi context window
  estimator untuk alias routing seperti `"auto"` (yang umum digunakan oleh gateway
  seperti LLMKita dengan target model 1M seperti GLM-5.3/Claude/GPT) agar tidak
  fallback ke batas konservatif 128k, melainkan teresolusi ke 1.000.000 tokens (1M).

### Release Process

- Bump versi ke 0.3.1 di seluruh manifest (workspace Cargo.toml, Tauri Cargo.toml,
  tauri.conf.json, package.json, kedua Cargo.lock)

## [0.3.0] - 2026-10-13

Minor release. 1 PR (#69) sejak v0.2.1.

### Added — Post-Turn Reflection Loop

- **Reflection engine** (`crates/maukode-core/src/reflection.rs`): setelah sebuah
  turn selesai, agent mengevaluasi performanya sendiri dan mengekstrak lessons
  yang durable dari percakapan yang telah diselesaikan.
- **`ReflectionConfig`**: opt-in via config dengan parameter:
  - `enabled` — master switch (default: false)
  - `min_tool_calls` — minimum jumlah tool calls dalam turn sebelum reflection
    triggers (default: 3), menghindari refleksi pada turn trivial
  - `skill_creation` — mengizinkan engine menulis skill files baru ke
    `.maukode/skills/learned/` (default: false)
- **Lesson extraction**: parsing output LLM untuk `[memory: title | taxonomy: X,
  content]` blocks, dengan taxonomy extraction (gotchas, conventions, dll)
- **Skill proposal**: parsing `SKILL:` blocks menjadi `ProposedSkill` dengan
  name, description, dan prompt
- **SessionRunner integration**: `with_reflection_config()` builder method
- **CLI flag**: `--reflection` untuk mengaktifkan reflection di TUI
- **Server API**: field `reflection` di `AgentConfig` dan `SetAgentConfigRequest`
  untuk mengatur reflection via REST API
- **Desktop Settings**: UI toggle untuk reflection config di Settings page
- **TS bindings**: `ReflectionConfig.ts` generated untuk desktop app

### Changed

- `AgentConfig` tidak lagi mengimplementasikan `Copy` (menambahkan field
  non-Copy `ReflectionConfig`). Handler server di-update untuk menggunakan
  `.clone()`.
- Workspace version bumped dari 0.2.1 ke 0.3.0

### Release Process

- Bump versi ke 0.3.0 di seluruh manifest (workspace Cargo.toml, Tauri Cargo.toml,
  tauri.conf.json, package.json, kedua Cargo.lock)

## [0.2.1] - 2026-10-13

Patch release. 1 PR (#66) sejak v0.2.0.

### Fixed

- **Compaction context window sizes**: Ganti hardcoded 200K-for-all-Claude dan
  128K default dengan ukuran context window per-model yang akurat, bersumber
  dari dokumentasi provider resmi:
  - **Claude**: 5 family (opus/sonnet/haiku/fable/mythos) + Opus 4.7/4.8 → 1M;
    Opus 4/4.5, Sonnet 4/4.5, Haiku 3.5, 3.x → 200K
  - **GPT**: GPT-6/5.4/5.6 → 1.050.000; GPT-5/5.1 → 400.000; GPT-4o → 128.000;
    GPT-4 → 8.192; GPT-4-32k → 32.768; GPT-3.5 → 16.384
  - **GLM**: GLM-5.x → 1.000.000; GLM-4.x → 131.072
  - Pattern matching mendukung notasi dashed (`claude-opus-4-8`) dan dotted
    (`claude-opus-4.8`)

### Release Process

- Bump versi ke 0.2.1 di seluruh manifest (workspace Cargo.toml, Tauri Cargo.toml,
  tauri.conf.json, package.json, kedua Cargo.lock)
- Tag `v0.2.1` sebelumnya dibuat tanpa release commit — versi masih 0.2.0.
  Perbaikan ini memastikan binary melaporkan versi yang benar.

## [0.2.0] - 2026-10-13

144 commit, 41 PR (#18–#64) sejak v0.1.0. ~27.000 baris kode baru.

### Added — Desktop App (Tauri + React)

- Aplikasi desktop native berbasis Tauri dengan WebView sistem (`apps/desktop/`)
- UI chat-style: sidebar session list, panel chat, composer dengan context
  mentions, token gauge, chips, dan footer affordances
- Migrasi ke Tailwind v4 + shadcn/ui (Radix UI + lucide icons)
- Manajemen session: pencarian, filter status, pinning, archive, rename, model
  switcher per-session
- Command Palette dan Shortcuts modal (navigasi keyboard-first)
- Settings page dengan Radix UI, plugins management, dan flow sign-in ChatGPT
- Review panel: per-file diffs, undo, worktree commit/merge/delete
- Subagent cards interaktif dengan trace drawer slide-over
- Native notifications dan repository picker
- External editor integration
- Project context (branch name, info project) di session list dan header
- Playwright e2e tests untuk desktop UI

### Added — Memory & Knowledge Store

- Crate baru `maukode-memory` dengan 7 modul:
  - Project-scoped knowledge store
  - FTS5 full-text search
  - Memory extraction otomatis dari konteks percakapan
  - Memory links (relasi antar knowledge items)
  - Memory tools (ekspos memory ke agent)
  - Scope & models
  - Error handling khusus

### Added — Security & Sandboxing

- Runtime command sandbox dengan mode: `off`, `path_jail`, `bubblewrap`, `auto`
- MCP server process sandboxing
- API endpoint untuk mengubah mode sandbox runtime
- Private tool review mode (gate akses ke likely secrets behind approval)
- DNS-rebinding Host guard untuk proteksi server

### Added — Subagent System

- SubagentTool implementation (child SessionRunner invocation)
- Subagent profiles: `codebase_researcher`, `security_auditor`
- MCP scoping & tool filtering untuk subagent
- Subagent registry (read-only) untuk discovery
- Server route `/api/v1/subagents` untuk list profiles
- Subagent events masuk ke global event feed

### Added — Skills Engine

- Backend skills engine dengan discovery dari markdown files
- Server endpoints untuk list dan manage skills
- SkillDefinition protocol schema

### Added — Plugin System

- Backend plugin system: discovery, loading, management
- Plugin management API (enable/disable)
- Plugin MCP server config

### Added — Plans, Timeline & Prompt Queuing

- Structured plans: PlanStep, Plan, StepStatus, FinishReason
- Timeline tracking step-by-step
- Prompt queuing & steering (QueueSteeringPrompt)
- Live plan checklist widget di TUI

### Added — Audit System

- Audit provenance viewer di UI
- Permission provenance (record semua permission decisions)
- Compaction provenance (record automatic compaction events)
- Persistent session turn identifiers
- SQLite tables untuk permission dan compaction audit

### Added — Preview System

- Sandboxed preview lifecycle API (#61): start/stop/refresh preview dev-server
- Live dev-server preview panel (#62): reverse proxy untuk preview web app
- PreviewInfo response: status, port, URL, readiness, error

### Added — Worktree & Parallel Agents

- Parallel-agent management dengan git worktrees
- Worktree status: commit/merge/delete dari UI
- Per-session model (setiap session bisa pakai model berbeda)

### Added — ChatGPT Authentication

- Sign in with ChatGPT provider (#18)
- ChatGPT auth flow (`chatgpt_auth.rs`, `chatgpt_client.rs`)
- Login lifecycle: login status, account management, use account

### Added — Server API

- SSE event replay dengan sequence numbers untuk reconnect
- Per-session permissions (permission mode per session, fork-able)
- Session titles (set/get)
- Structured audit trail
- Parallel-agent API: worktree, per-session model, global feed
- DNS-rebinding guard (Host header validation)
- Routes: auth, events, plugins, preview, settings, skills

### Added — CI & Release Infrastructure

- Linux build workflow (`linux-build.yml`): tarball release untuk x86_64,
  smoke test under Xvfb, upload artifact
- Desktop CI checks: compile + smoke-test Tauri shell
- Self-hosted runner (#57)
- Blacksmith build cache
- TS bindings sync via `cargo test --features ts`

### Added — TUI Enhancements

- Markdown rendering dengan pulldown-cmark (`markdown.rs`)
- Render cache (`render_cache.rs`) untuk stop O(history) redraws
- Loading animations
- Turn cancellation (cancel generation mid-stream)
- Idle redraw skip (hemat CPU)
- Subagent trace inspector modal
- Plan checklist widget

### Changed

- Workspace version bumped dari 0.1.0 ke 0.2.0
- Desktop app migrated dari native HTML selects ke Radix UI components
- Server event system menggunakan sequence-numbered SSE replay

### Fixed

- Agent never ends turn silently (push model untuk finish tasks)
- LLM stream tolerance: EOF, flaky SSE streams, retry transient failures
- SQLite lock contention saat first open (flaky CI fix)
- Store cleanup: empty sessions, prevent `read_file` panic (#53)
- Send bound fix: box-pin `start_turn` future untuk ensure `Send`
- Multiple desktop UI fixes: cropping, focus, sidebar toggle, localStorage
  guard, platform helpers
- PreviewRequest TypeScript type: add missing `sandbox_mode` field (#63)
- Tauri Cargo.lock out-of-date untuk `--locked` release build (#64)

## [0.1.0] - 2026-10-03

Initial release.
