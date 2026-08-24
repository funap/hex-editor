# xvw

A fast, GPU-accelerated hex editor written in Rust, built on [GPUI](https://gpui.rs/) (the UI framework behind Zed).

Designed to be snappy and practical for inspecting binary files, custom formats, and firmware blobs.

## Features

- **Fast & large file support** — Uses `memmap2` under the hood so multi-GB files open instantly with minimal memory overhead.
- **Kaitai Struct support** — Load `.ksy` format definitions at runtime (`Cmd+Shift+S` / `Ctrl+Shift+S`) to parse and navigate binary structures interactively.
- **Custom line breaks** — Hit `Enter` anywhere to break a line at logical boundaries (packet headers, struct records) instead of being locked into a rigid 16-byte grid. Use `Cmd+J` / `Ctrl+J` to join lines.
- **Data inspector** — Click any byte to inspect it decoded simultaneously as integers (i8–i64, u8–u64, big/little endian), floats (f32, f64), timestamps, hex, and text.
- **Side-by-side diff** — Compare two binary files with synchronized scrolling and difference highlights.
- **2D Visual map / entropy view** — Visualize byte distribution in 2D with grayscale, data category, and rainbow color modes to spot compressed/encrypted sections.
- **Vim keybindings** — `h`/`j`/`k`/`l` movement, `Shift+H/J/K/L` selection, `/` search with hex, text, and regex modes.
- **Export / Copy As** — One-click copy to C array, Rust array, JSON, Base64, Hex dump, escaped strings, etc.
- **40+ Text encodings** — Decode and search binary data across a comprehensive selection of character sets:
  - **Unicode & ASCII**: UTF-8, UTF-16 LE, UTF-16 BE, ASCII
  - **Japanese**: Shift-JIS (CP932 / Windows-31J), EUC-JP, ISO-2022-JP
  - **Chinese & Korean**: GBK, GB18030, Big5, EUC-KR
  - **ISO-8859 Family**: ISO-8859-1 through ISO-8859-16 (Latin 1–10, Cyrillic, Arabic, Greek, Hebrew, Celtic, etc.)
  - **Windows Code Pages**: Windows-1250 through Windows-1258
  - **Legacy / DOS / Mac**: KOI8-R, KOI8-U, Mac OS Roman, IBM866
- **Flexible display** — Switch radix (hex, dec, oct, bin) and byte grouping (1, 2, 4, 8 bytes) to match your workflow.

## Getting Started

Requires Rust (edition 2024 / latest stable).

```bash
# Clone the repository
git clone https://github.com/funap/hex-viewer.git
cd hex-viewer

# Run directly
cargo run

# Or pass a file / directory
cargo run -- path/to/binary_file.bin
cargo run -- path/to/folder
```

## Keybindings

### Navigation & Editing

| Shortcut (macOS) | Shortcut (Linux / Win) | Action |
|---|---|---|
| `h` / `j` / `k` / `l` | `h` / `j` / `k` / `l` | Move cursor |
| `Shift + h/j/k/l` | `Shift + h/j/k/l` | Expand selection |
| `Home` / `Cmd+Home` | `Home` / `Ctrl+Home` | Jump to start of file |
| `End` / `Cmd+End` | `End` / `Ctrl+End` | Jump to end of file |
| `Cmd+L` / `Ctrl+G` | `Ctrl+L` / `Ctrl+G` | Go to offset / address |
| `Enter` | `Enter` | Insert custom line break |
| `Cmd+J` | `Ctrl+J` | Join lines |
| `Cmd+Shift+Backspace` | `Ctrl+Shift+Backspace` | Reset custom line breaks |

### Search & Workspace

| Shortcut (macOS) | Shortcut (Linux / Win) | Action |
|---|---|---|
| `/` or `Cmd+F` | `/` or `Ctrl+F` | Search |
| `n` / `Cmd+G` | `n` / `F3` | Next search match |
| `Shift+N` / `Cmd+Shift+G` | `Shift+N` / `Shift+F3` | Previous search match |
| `Cmd+O` | `Ctrl+O` | Open file |
| `Cmd+Shift+O` | `Ctrl+Shift+O` | Open folder |
| `Cmd+B` | `Ctrl+B` | Toggle left sidebar |
| `Cmd+\` / `Cmd+Shift+D` | `Ctrl+\` / `Ctrl+Shift+D` | Split editor (right / down) |
| `Cmd+1` .. `Cmd+9` | `Ctrl+1` .. `Ctrl+9` | Switch tab |
| `Cmd+Shift+S` | `Ctrl+Shift+S` | Load Kaitai Struct (`.ksy`) |
| `Cmd+Shift+V` | `Ctrl+Shift+V` | Toggle inline structure view |
| `Cmd+Shift+C` | `Ctrl+Shift+C` | Copy as hex dump |

## License

MIT
