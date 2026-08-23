<div align="center">
  <img src="https://raw.githubusercontent.com/rust-lang/rust-artwork/master/logo/rust-logo-512-blk.png" width="96" height="96" alt="XVW Logo" />

  <h1>XVW</h1>
  <p><strong>A high-performance, modern binary & hex editor built with Rust and GPUI.</strong></p>

  <p>
    <a href="https://github.com/rust-lang/rust"><img src="https://img.shields.io/badge/Rust-2024_Edition-orange.svg?logo=rust&style=flat-square" alt="Rust Version" /></a>
    <a href="https://gpui.rs/"><img src="https://img.shields.io/badge/UI_Framework-GPUI-blue.svg?style=flat-square" alt="GPUI Framework" /></a>
    <a href="https://tokio.rs/"><img src="https://img.shields.io/badge/Async-Tokio-007ACC.svg?style=flat-square" alt="Tokio Async" /></a>
    <a href="https://github.com/kaitai-io/kaitai_struct"><img src="https://img.shields.io/badge/Parser-Kaitai_Struct-brightgreen.svg?style=flat-square" alt="Kaitai Struct" /></a>
    <a href="https://github.com/memmap2-rs/memmap2"><img src="https://img.shields.io/badge/I%2FO-Memory_Mapped-yellowgreen.svg?style=flat-square" alt="Memory Mapped" /></a>
  </p>

  <h4>⚡ Fast. GPU-Accelerated. Interactive Structure Parsing. Intuitive. ⚡</h4>
  
  ---
</div>

## 📖 Overview

**XVW** (*X-View*) is a next-generation binary and hex editor designed for reverse engineers, security researchers, firmware engineers, and system developers.

Built on top of the ultra-fast **GPUI framework** (the hardware-accelerated UI toolkit powering the Zed editor) and backed by **Rust** and **Tokio**, XVW combines the blistering speed of memory-mapped native execution with a sleek, modern IDE-like interface.

Whether analyzing proprietary firmware, reverse engineering protocol packets, inspecting multi-gigabyte disk images, or diffing binaries, XVW offers a complete suite of professional binary exploration tools.

---

## ✨ Key Features

### 🏎️ GPU-Accelerated Fluid Rendering & Memory-Mapped I/O
- **Instantaneous 120 FPS Rendering:** Leveraging GPUI's hardware-accelerated rendering pipeline for jitter-free, smooth scrolling.
- **Large File Support:** Employs zero-copy memory mapping (`memmap2`) to open and navigate multi-gigabyte binary files instantly without memory bottlenecks.

### 🧬 Dynamic Binary Structure Parsing (Kaitai Struct)
- **Runtime `.ksy` Loading:** Load Kaitai Struct YAML format specifications on the fly (`Cmd+Shift+S`) without rebuilding or restarting.
- **Interactive Structure Tree:** Explore complex nested headers, variable-length fields, bit flags, and sub-structures.
- **Bi-directional Navigation:** Clicking any field in the structure tree instantly focuses and highlights the corresponding byte range in the Hex View, and vice-versa.
- **Inline Structure View:** Toggle inline annotations directly within the hex buffer (`Cmd+Shift+V`).

### 🔍 Real-Time Data Inspector
- Click anywhere in the hex grid to inspect the byte sequence interpreted across multiple types simultaneously:
  - **Integers:** `int8` / `uint8`, `int16` / `uint16`, `int32` / `uint32`, `int64` / `uint64`
  - **Floating Point:** `float32` (Single precision), `float64` (Double precision)
  - **Timestamps:** 32-bit & 64-bit Unix Epoch to formatted UTC Date & Time
  - **Text Encodings:** ASCII, UTF-8, UTF-16
  - **Hex Representations:** 8-bit, 16-bit, 32-bit, 64-bit
- Supports instant switching between **Little Endian** and **Big Endian** byte ordering.

### 🗺️ 2D Visual Map & Entropy Visualization
- Visualize binary data distributions across configurable columns and pixel scales.
- Multiple color modes:
  - **Grayscale:** Density and raw byte magnitude.
  - **Data Category:** Instantly distinguish null bytes, ASCII printable text, control characters, and high-byte sequences.
  - **Rainbow:** High-contrast spectrum for locating embedded assets, encrypted blocks, and compressed data streams.

### 🎨 8-Color Multi-Layer Bookmarks & Annotation Management
- Bookmark byte ranges in 8 distinct colors (Red, Orange, Yellow, Green, Cyan, Blue, Purple, Pink).
- Dedicated **Bookmarks Panel** to list, filter, jump between, and delete bookmarks.
- **Import / Export Bookmarks:** Save and share analysis bookmarks as JSON files across sessions.

### 📐 Custom Line Breaks & Adaptive Grid Formatting
Break free from rigid 16-byte hex rows:
- **Custom Line Breaks:** Insert custom line breaks (`Enter`) to align lines with logical packet or struct boundaries.
- **Join Lines (`Shift+J`):** Combine lines dynamically.
- **Reset Layout (`Cmd+Shift+Backspace`):** Revert back to the standard grid layout instantly.

### 🌓 Universal Radix, Grouping & Encoding Engine
- **Radix Switching:** Hexadecimal (Base 16), Decimal (Base 10), Octal (Base 8), and Binary (Base 2).
- **Byte Grouping:** Group columns by 1 Byte (8-bit), 2 Bytes (16-bit), 4 Bytes (32-bit), or 8 Bytes (64-bit).
- **Text Encodings:** Real-time side-by-side interpretation in ASCII, UTF-8, UTF-16 LE, and UTF-16 BE.

### ⚖️ Side-by-Side Binary Diffing
- Compare two binary files with synchronized dual-pane scrolling.
- Visual difference indicators highlighting insertions, deletions, and modifications.
- Step through differences sequentially (`Next Difference` / `Prev Difference`).

### 🧮 Checksum & Hash Calculation Panel
- Real-time checksum calculations over whole files or active selections:
  - **Sums:** Sum 8-bit, Sum 16-bit, Sum 32-bit, Sum 64-bit
  - **CRCs:** CRC-16 (CCITT, ARC), CRC-32, Adler-32
  - **Cryptographic Hashes:** MD5, SHA-256

### 📋 Rich "Copy As" Export Formats
Copy selected bytes in one click into ready-to-use formats:
- **Hex Dump** (`Cmd+Shift+C`)
- **C/C++ Array** (`const uint8_t data[] = { ... }`)
- **Rust Array** (`const DATA: [u8; N] = [ ... ]`)
- **JSON Array** (`[0, 255, ...]`)
- **Base64 String**
- **Hex Stream** (`00FF...`) & **Hex with Spaces** (`00 FF ...`)
- **Escaped String** (`\x00\xFF...`)
- **Printable Text & Binary Strings**

### 🖥️ Modern IDE Workspace & Vim Keybindings
- **Tab & Multi-Pane Layout:** Open multiple files in tabs, with horizontal (`Cmd+\`) and vertical (`Cmd+Shift+D`) split panes.
- **Collapsible Sidebar:** Access File Tree, Kaitai Structure, Bookmarks, and Checksum panels via the Activity Bar.
- **Vim Navigation:** Use `h`, `j`, `k`, `l` to move, `Shift+H/J/K/L` to expand selection, `/` to search, and `n` / `N` for next/previous match.
- **Full Search Support:** Incremental search with Hex, Text, and Regex modes.

---

## ⌨️ Keybindings Cheat Sheet

### Navigation & Cursor Movement
| Shortcut | Action |
|---|---|
| `h` / `Left` | Move Left |
| `l` / `Right` | Move Right |
| `k` / `Up` | Move Up |
| `j` / `Down` | Move Down |
| `Shift + h/j/k/l` | Expand Selection |
| `Home` / `Cmd+Home` | Jump to Beginning of File |
| `End` / `Cmd+End` | Jump to End of File |
| `PageUp` / `PageDown` | Page Scroll |

### Search & Layout
| Shortcut | Action |
|---|---|
| `Cmd+F` / `/` | Toggle Search Bar |
| `Cmd+G` / `n` / `F3` | Next Search Match |
| `Cmd+Shift+G` / `Shift+N` / `Shift+F3` | Previous Search Match |
| `Enter` | Insert Custom Line Break |
| `Shift+J` | Join Line |
| `Backspace` / `Delete` | Remove Line Break (Backward / Forward) |
| `Cmd+Shift+Backspace` | Reset All Custom Breaks |

### Structure & Workspace
| Shortcut | Action |
|---|---|
| `Cmd+O` | Open File Dialog |
| `Cmd+Shift+O` | Open Directory / Folder |
| `Cmd+B` | Toggle Left Sidebar |
| `Cmd+W` / `Ctrl+W` | Close Active Tab |
| `Cmd+\` | Split Editor Pane Right |
| `Cmd+Shift+D` | Split Editor Pane Down |
| `Cmd+1` .. `Cmd+9` | Switch to Tab 1–9 |
| `Cmd+Shift+S` | Load Kaitai Structure Definition (`.ksy`) |
| `Cmd+Shift+V` | Toggle Inline Structure View |
| `Cmd+,` | Open Settings |
| `Cmd+Shift+C` | Copy Selection as Hex Dump |

---

## 🚀 Getting Started

### Prerequisites
- [Rust](https://rustup.rs/) (edition 2024 / latest stable)

### Build & Run

```bash
# Clone the repository
git clone https://github.com/your-username/xvw.git
cd xvw

# Run directly
cargo run

# Or open a specific file or folder directly from command line
cargo run -- path/to/binary_file.bin
cargo run -- path/to/folder

# Build an optimized release binary
cargo build --release

# Run the unit test suite
cargo test
```

---

## 📄 License

This project is licensed under the MIT License / Apache-2.0. See LICENSE for details.
