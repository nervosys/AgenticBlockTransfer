# AI Ontology

`abt ontology --full` emits a complete [JSON-LD](https://json-ld.org/) capability schema using [schema.org](https://schema.org/) vocabulary.

## Purpose

An AI agent can call `abt ontology --full` once to learn everything it needs to operate `abt` — parameters, types, constraints, defaults, examples, exit codes, preconditions, postconditions — without reading documentation.

## Output Formats

| Format  | Command                   |
| ------- | ------------------------- |
| JSON-LD | `abt ontology --full`     |
| JSON    | `abt ontology -f json`    |
| YAML    | `abt ontology -f yaml`    |
| OpenAPI | `abt ontology -f openapi` |

## Schema Coverage

- **7 capabilities** — write, verify, list, info, checksum, format, ontology
- **Type definitions** — image formats, compression, device types, filesystems, hash algorithms
- **Platform matrix** — OS-specific device paths, elevation methods
- **Device scope** — 4 categories: microcontroller, removable, desktop, cloud
- **Exit codes** — structured error semantics for automation

## Example: Agent Workflow

```python
import subprocess, json

# 1. Discover capabilities
ontology = json.loads(subprocess.check_output(["abt", "ontology", "-f", "json"]))

# 2. List devices
devices = json.loads(subprocess.check_output(["abt", "list", "--json"]))

# 3. Write with safety token
target = devices["devices"][0]
subprocess.run([
    "abt", "write",
    "-i", "image.iso",
    "-o", target["path"],
    "--confirm-token", target["confirm_token"],
    "--safety-level", "cautious",
    "--output", "json"
])
```
