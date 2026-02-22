# wstui

This is very much a work in progress.

A **WhatsApp client** for the terminal. Built in Rust with [ratatui](https://github.com/ratatui-org/ratatui), it uses the [whatsmeow](https://github.com/tulir/whatsmeow) backend and gives you a keyboard-driven, vim-style TUI to chat.

## Requirements

- **Rust** (2024 edition; recent stable toolchain)
- **Go** (Testet with 1.26.0)

## Building

```bash
cargo build --release
```

## Usage


```bash
# Start and link with QR code (shown in terminal)
cargo run

# Link with phone number (pairing code will be printed)
cargo run -- -p +1234567890
```

```bash
# Start and link with QR code (shown in terminal)
./target/release/wstui

# Link with phone number (pairing code will be printed)
./target/release/wstui --phone +1234567890
```

On first run the client creates `whatsmeow_store.db` (session) and uses a `media/` directory for downloaded files. The local message cache is in `whatsapp.db`.

## Keybindings

|General| |
|-------|----|
| Quit | `Ctrl+Q` |
| Toggle logs | `Ctrl+Shift+L` |

| Focus / navigation | |
|-------|----|
| Chat list → Message list | `Ctrl+L` (when focus is on chat list) |
| Message list → Input | `Ctrl+J` |
| Input → Message list | `Ctrl+K` |
| Input → Chat list | `Ctrl+H` |
| Message list → Chat list | `Ctrl+H` |

| **Chat list** | |
|-------|----|
| Next / previous chat | `j` / `k` |
| Open chat (focus input) | `Enter` |

| **Message list** | |
|-------|----|
| Next / previous message | `j` / `k` |
| First / last message | `G` / `g g` |
| Scroll view | `Ctrl+E` / `Ctrl+Y` |
| Reply to selected message | `r` |
| Go to quoted message | `g q` |
| Open message view (full content) | `Enter` |

| **Message view** | |
|-------|----|
| Close back to list | `Esc` |

| Input | |
|-------|----|
| Send message | `Ctrl+X` |
| Clear quote | `Ctrl+R` |
| **Input (Vim)** | `i` Insert, `Esc` Normal, then h/j/k/l, w/e/b, ^/$, d/c/p/y, etc. |

Image protocol (halfblocks vs sixels) can be cycled with `Ctrl+P` if your terminal supports it.
