# TUI Mode

Launch the interactive terminal UI with:

```bash
abt tui
```

## Navigation

| Key     | Action                |
| ------- | --------------------- |
| `Tab`   | Switch between panels |
| `↑`/`↓` | Navigate lists        |
| `Enter` | Select / confirm      |
| `Esc`   | Cancel / go back      |
| `q`     | Quit                  |

## Panels

1. **Source** — text input for image path, or press `Tab` to open the file browser
2. **Devices** — table of available block devices with type, size, and model
3. **Write** — confirmation prompt with safety report, then progress gauge

## File Browser

The built-in file browser supports:
- Directory navigation with `↑`/`↓` and `Enter`
- Extension filtering for disk image files
- Parent directory navigation with `..`
- Current path display in the header
