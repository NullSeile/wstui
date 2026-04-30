# wstui

This is very much a work in progress.

A **WhatsApp client** for the terminal. Built in Rust with [ratatui](https://github.com/ratatui-org/ratatui), it uses the [whatsmeow](https://github.com/tulir/whatsmeow) backend and gives you a keyboard-driven, vim-style TUI to chat.

## Requirements

- **Rust** (2024 edition; recent stable toolchain)
- **Go** (Testet with 1.26.0)

## Installing
### Arch
There is an official AUR package that provides the `wstui` command:
- [wstui-git](https://aur.archlinux.org/packages/wstui-git): last commit of `master`
- [wstui](https://aur.archlinux.org/packages/wstui): latest release

If you are not using arch, follow the building and usage sections.

### Installing from source

```bash
git clone https://github.com/NullSeile/wstui
cd wstui
cargo install --path .
```

## Usage
```bash
# Start and link with QR code (shown in terminal)
wstui

# Link with phone number (pairing code will be printed)
wstui -- -p +1234567890
```

On first run the client creates `whatsmeow_store.db` (session) and uses a `media/` directory for downloaded files. The local message cache is in `whatsapp.db`.

## Keybindings

|General| |
|-------|----|
| Quit | `Ctrl+Q` |
| Toggle logs | `Ctrl+Shift+L` |
| Cycle image protocol | `Ctrl+P` |

|Focus / navigation| |
|-------|----|
| Chat list → Message list | `Ctrl+L` |
| Message list → Chat list | `Ctrl+H` |
| Message list → Input | `Ctrl+J` |
| Input → Message list | `Ctrl+K` |
| Input → Chat list | `Ctrl+H` |
| Message view → Message list | `Esc` |

| **Chat list** | |
|-------|----|
| Next / previous chat | `j` / `k` |
| Open chat | `Enter` or `l` |
| Search contacts | `/` |
| Move cursor left/right | `←` / `→` |

| **Message list** | |
|-------|----|
| Next / previous message | `j` / `k` |
| First message | `g g` |
| Last message | `G` |
| Scroll up | `Ctrl+E` |
| Scroll down | `Ctrl+Y` |
| Open (external) | `o` |
| Reply to message | `r` |
| Copy to clipboard | `y` |
| View full content | `Enter` |
| Go to quoted message | `g q` |
| Reset selection | `Esc` |

| **Input** | |
|-------|----|
| Send message | `Ctrl+X` |
| Edit with external editor | `Ctrl+E` |
| Clear quote | `Space r` |
| Attach image | `Space a i` |
| Attach document | `Space a d` |
| Clear attachment | `Space a r` |
| Paste from clipboard | `Space p` |

| **Input (Vim mode)** | |
|-------|----|
| Enter insert mode | `i` |
| Enter normal mode | `Esc` |
| Append | `a` |
| Append at EOL | `A` |
| New line below | `o` |
| New line above | `O` |
| Move left | `h` |
| Move down | `j` |
| Move up | `k` |
| Move right | `l` |
| Word forward | `w` |
| Word end | `e` |
| Word back | `b` |
| Line start | `^` |
| Line end | `$` |
| Yank (copy) line | `yy` |
| Delete line | `dd` |
| Change line | `cc` |
| Paste | `p` |
| Undo | `u` |
| Redo | `Ctrl+R` |
| Delete char | `x` |
| Visual mode | `v` |
| Visual line | `V` |
| Visual yank | `y` |
| Visual delete | `d` |
| Visual change | `c` |
| Scroll down | `Ctrl+D` |
| Scroll up | `Ctrl+U` |
| Page down | `Ctrl+F` |
| Page up | `Ctrl+B` |
| Go to top | `g g` |
| Go to bottom | `G` |
