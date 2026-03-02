# MCP Server

`abt mcp` starts a [Model Context Protocol](https://modelcontextprotocol.io/) server for direct AI agent integration.

## Starting the Server

```bash
# Persistent server (reads JSON-RPC messages from stdin)
abt mcp

# Single-request mode (process one message and exit)
abt mcp --oneshot
```

## Protocol

- **Transport**: JSON-RPC 2.0 over stdio (stdin/stdout)
- **Tools**: 6 tools exposed to the AI agent

## Available Tools

| Tool       | Description                |
| ---------- | -------------------------- |
| `write`    | Write an image to a device |
| `verify`   | Verify written data        |
| `list`     | List available devices     |
| `info`     | Inspect a device or image  |
| `checksum` | Compute file checksums     |
| `format`   | Format a device            |

## Integration with AI Frameworks

The MCP server works with any MCP-compatible AI framework. The agent sends tool invocation requests as JSON-RPC messages to `abt`'s stdin, and receives structured responses on stdout.
