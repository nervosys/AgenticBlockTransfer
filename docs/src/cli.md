# CLI Reference

The CLI is the primary interface, available on all platforms and feature configurations.

## Argument Parsing

`abt` uses [clap](https://docs.rs/clap) with derive macros for type-safe argument parsing. All arguments support:

- **Long flags** — `--input`, `--output`, `--block-size`
- **Short flags** — `-i`, `-o`, `-b`
- **Environment variables** — `ABT_BLOCK_SIZE`, `ABT_SAFETY_LEVEL`
- **Aliases** — `abt flash` = `abt write`, `abt hash` = `abt checksum`

## JSON Output

All commands support `--output json` for structured machine-readable output:

```bash
abt list --output json
abt write -i image.iso -o /dev/sdb --output json --dry-run
abt checksum image.iso --output json
```

## Piping and Scripting

```bash
# Get device path from JSON output
DEVICE=$(abt list --json | jq -r '.devices[0].path')
TOKEN=$(abt list --json | jq -r '.devices[0].confirm_token')

# Write with token
abt write -i image.iso -o "$DEVICE" --confirm-token "$TOKEN" --force
```
