# Installation

## From Source (Recommended)

Requires Rust 1.75+.

```bash
git clone https://github.com/nervosys/AgenticBlockTransfer.git
cd AgenticBlockTransfer
cargo build --release
```

Binary: `target/release/abt` (`abt.exe` on Windows).

## Package Managers

### Cargo (crates.io)

```bash
cargo install abt
```

### Homebrew (macOS/Linux)

```bash
brew tap nervosys/tap
brew install abt
```

### Winget (Windows)

```powershell
winget install nervosys.abt
```

### Arch Linux (AUR)

```bash
yay -S abt
```

### Debian/Ubuntu

```bash
sudo dpkg -i abt_1.0.0_amd64.deb
```

### Fedora/RHEL

```bash
sudo rpm -i abt-1.0.0-1.x86_64.rpm
```

## Feature Flags

| Feature | Default | Description                               |
| ------- | ------- | ----------------------------------------- |
| `cli`   | ✅       | Command-line interface (always available) |
| `tui`   | ✅       | Terminal UI via ratatui + crossterm       |
| `gui`   | ✅       | Native GUI via egui/eframe                |

CLI-only build (smaller binary):

```bash
cargo build --release --no-default-features --features cli
```

## Shell Completions

```bash
# Bash
abt completions bash > /etc/bash_completion.d/abt

# Zsh
abt completions zsh > ~/.zsh/completions/_abt

# Fish
abt completions fish > ~/.config/fish/completions/abt.fish

# PowerShell
abt completions powershell | Out-String | Invoke-Expression
```

## Man Pages

```bash
abt man --output-dir /usr/local/share/man/man1/
```
