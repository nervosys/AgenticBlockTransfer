# Configuration

`abt` supports a TOML configuration file for persistent defaults.

## Config File Location

| Platform | Path                                            |
| -------- | ----------------------------------------------- |
| Linux    | `~/.config/abt/config.toml`                     |
| macOS    | `~/Library/Application Support/abt/config.toml` |
| Windows  | `%APPDATA%\abt\config.toml`                     |

## Example Configuration

```toml
[write]
block_size = "4M"
verify = true
sparse = false
direct_io = false

[safety]
level = "cautious"
backup_partition_table = true

[output]
format = "text"    # "text" or "json"

[logging]
level = "info"
file = ""          # path to JSON log file
```

## Precedence

1. Command-line flags (highest priority)
2. Environment variables
3. Config file
4. Built-in defaults (lowest priority)
