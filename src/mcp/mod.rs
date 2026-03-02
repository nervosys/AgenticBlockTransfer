// MCP (Model Context Protocol) server — JSON-RPC 2.0 over stdio
//
// Exposes abt capabilities as MCP tools so that AI agents can discover and
// invoke block-transfer operations through the standard MCP protocol.
//
// Usage:  abt mcp            (starts stdio JSON-RPC server)
//         abt mcp --oneshot  (process a single request and exit)
//
// Protocol: https://modelcontextprotocol.io/specification
//
// Tools exposed:
//   abt_list_devices   — enumerate block devices
//   abt_info            — inspect image or device
//   abt_checksum        — compute file/device hash
//   abt_write           — write image to device (destructive)
//   abt_verify          — verify written data
//   abt_format          — format device with filesystem (destructive)

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

/// MCP protocol version we implement.
const MCP_PROTOCOL_VERSION: &str = "2024-11-05";

/// Server info returned in initialize response.
const SERVER_NAME: &str = "abt";

// ─── JSON-RPC Types ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    jsonrpc: String,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    code: i64,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

impl JsonRpcResponse {
    fn success(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: Some(result),
            error: None,
        }
    }

    fn error(id: Value, code: i64, message: String) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message,
                data: None,
            }),
        }
    }

    fn method_not_found(id: Value, method: &str) -> Self {
        Self::error(id, -32601, format!("Method not found: {}", method))
    }

    fn invalid_params(id: Value, msg: String) -> Self {
        Self::error(id, -32602, msg)
    }
}

// ─── MCP Tool Definitions ──────────────────────────────────────────────────

fn tool_definitions() -> Value {
    json!([
        {
            "name": "abt_list_devices",
            "description": "List available block devices / storage targets. Returns device paths, sizes, types, and whether they are removable or system drives.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "all": {
                        "type": "boolean",
                        "description": "Include system drives (default: false)",
                        "default": false
                    },
                    "removable": {
                        "type": "boolean",
                        "description": "Show only removable devices (default: false)",
                        "default": false
                    },
                    "device_type": {
                        "type": "string",
                        "description": "Filter by device type: usb, sd, nvme, sata, etc.",
                        "enum": ["usb", "sd", "nvme", "sata", "scsi", "mmc", "emmc"]
                    }
                },
                "required": []
            }
        },
        {
            "name": "abt_info",
            "description": "Show detailed information about a device or image file. For images: format, size, compression, ISO metadata, QCOW2/VHD headers. For devices: type, size, partitions.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Device path or image file path to inspect"
                    }
                },
                "required": ["path"]
            }
        },
        {
            "name": "abt_checksum",
            "description": "Compute cryptographic hash of a file or device. Supports MD5, SHA-1, SHA-256, SHA-512, BLAKE3, CRC32.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File or device to hash"
                    },
                    "algorithm": {
                        "type": "string",
                        "description": "Hash algorithm (default: sha256)",
                        "enum": ["md5", "sha1", "sha256", "sha512", "blake3", "crc32"],
                        "default": "sha256"
                    }
                },
                "required": ["path"]
            }
        },
        {
            "name": "abt_write",
            "description": "Write an image to a target device or file. DESTRUCTIVE: all data on the target will be overwritten. Supports raw, compressed (gz/bz2/xz/zstd/zip), ISO, QCOW2, and VHD formats.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "source": {
                        "type": "string",
                        "description": "Source image file path or URL"
                    },
                    "target": {
                        "type": "string",
                        "description": "Target device path (e.g., /dev/sdb, \\\\.\\PhysicalDrive1)"
                    },
                    "verify": {
                        "type": "boolean",
                        "description": "Verify after writing (default: true)",
                        "default": true
                    },
                    "hash_algorithm": {
                        "type": "string",
                        "description": "Hash algorithm for verification (default: sha256)",
                        "default": "sha256"
                    },
                    "block_size": {
                        "type": "string",
                        "description": "Block size for I/O (e.g., 4M, 1M, 512K; default: 4M)",
                        "default": "4M"
                    },
                    "sparse": {
                        "type": "boolean",
                        "description": "Skip all-zero blocks (default: false)",
                        "default": false
                    },
                    "force": {
                        "type": "boolean",
                        "description": "Skip safety confirmation (required for unattended use)",
                        "default": false
                    },
                    "safety_level": {
                        "type": "string",
                        "description": "Safety level: low, medium, high",
                        "enum": ["low", "medium", "high"],
                        "default": "medium"
                    },
                    "dry_run": {
                        "type": "boolean",
                        "description": "Run pre-flight checks without writing",
                        "default": false
                    }
                },
                "required": ["source", "target"]
            }
        },
        {
            "name": "abt_verify",
            "description": "Verify written data against source image or expected hash value.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "target": {
                        "type": "string",
                        "description": "Target device or file to verify"
                    },
                    "source": {
                        "type": "string",
                        "description": "Source image file for comparison"
                    },
                    "expected_hash": {
                        "type": "string",
                        "description": "Expected hash value (alternative to source comparison)"
                    },
                    "hash_algorithm": {
                        "type": "string",
                        "description": "Hash algorithm (default: sha256)",
                        "default": "sha256"
                    }
                },
                "required": ["target"]
            }
        },
        {
            "name": "abt_format",
            "description": "Format a device with a filesystem. DESTRUCTIVE: all data will be erased. Supports FAT16, FAT32, exFAT, NTFS, ext2/3/4, XFS, Btrfs.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "device": {
                        "type": "string",
                        "description": "Device path to format"
                    },
                    "filesystem": {
                        "type": "string",
                        "description": "Filesystem type",
                        "enum": ["fat16", "fat32", "exfat", "ntfs", "ext2", "ext3", "ext4", "xfs", "btrfs"]
                    },
                    "label": {
                        "type": "string",
                        "description": "Volume label (optional)"
                    },
                    "quick": {
                        "type": "boolean",
                        "description": "Quick format (default: false)",
                        "default": false
                    },
                    "force": {
                        "type": "boolean",
                        "description": "Skip safety confirmation",
                        "default": false
                    }
                },
                "required": ["device", "filesystem"]
            }
        }
    ])
}

// ─── MCP Server ────────────────────────────────────────────────────────────

/// MCP server state.
pub struct McpServer {
    initialized: bool,
}

impl McpServer {
    pub fn new() -> Self {
        Self { initialized: false }
    }

    /// Handle a single JSON-RPC request and produce a response.
    pub fn handle_request(&mut self, request: &JsonRpcRequest) -> Option<JsonRpcResponse> {
        let id = request.id.clone().unwrap_or(Value::Null);

        match request.method.as_str() {
            // ── Lifecycle ──
            "initialize" => {
                self.initialized = true;
                Some(JsonRpcResponse::success(
                    id,
                    json!({
                        "protocolVersion": MCP_PROTOCOL_VERSION,
                        "capabilities": {
                            "tools": { "listChanged": false }
                        },
                        "serverInfo": {
                            "name": SERVER_NAME,
                            "version": env!("CARGO_PKG_VERSION")
                        }
                    }),
                ))
            }

            "notifications/initialized" => {
                // Client acknowledgement — no response needed
                None
            }

            "ping" => Some(JsonRpcResponse::success(id, json!({}))),

            // ── Tool discovery ──
            "tools/list" => Some(JsonRpcResponse::success(
                id,
                json!({ "tools": tool_definitions() }),
            )),

            // ── Tool invocation ──
            "tools/call" => {
                let tool_name = request.params.get("name").and_then(|v| v.as_str());
                let arguments = request.params.get("arguments").cloned().unwrap_or(json!({}));

                match tool_name {
                    Some(name) => {
                        let result = self.execute_tool(name, &arguments);
                        Some(JsonRpcResponse::success(id, result))
                    }
                    None => Some(JsonRpcResponse::invalid_params(
                        id,
                        "Missing 'name' in tools/call params".into(),
                    )),
                }
            }

            // ── Unknown method ──
            _ => {
                // Notifications (no id) don't get a response
                if request.id.is_some() {
                    Some(JsonRpcResponse::method_not_found(id, &request.method))
                } else {
                    None
                }
            }
        }
    }

    /// Execute a tool and return the MCP content result.
    fn execute_tool(&self, name: &str, args: &Value) -> Value {
        match name {
            "abt_list_devices" => self.tool_list_devices(args),
            "abt_info" => self.tool_info(args),
            "abt_checksum" => self.tool_checksum(args),
            "abt_write" => self.tool_write(args),
            "abt_verify" => self.tool_verify(args),
            "abt_format" => self.tool_format(args),
            _ => json!({
                "content": [{
                    "type": "text",
                    "text": format!("Unknown tool: {}", name)
                }],
                "isError": true
            }),
        }
    }

    // ── Tool implementations ───────────────────────────────────────────

    fn tool_list_devices(&self, args: &Value) -> Value {
        let all = args.get("all").and_then(|v| v.as_bool()).unwrap_or(false);
        let removable = args
            .get("removable")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Build the equivalent CLI command
        let mut cmd_parts = vec!["abt", "list", "--json"];
        if all {
            cmd_parts.push("--all");
        }
        if removable {
            cmd_parts.push("--removable");
        }

        self.execute_cli_and_capture(&cmd_parts)
    }

    fn tool_info(&self, args: &Value) -> Value {
        let path = match args.get("path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => {
                return self.error_content("Missing required parameter: path");
            }
        };
        self.execute_cli_and_capture(&["abt", "info", path])
    }

    fn tool_checksum(&self, args: &Value) -> Value {
        let path = match args.get("path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => {
                return self.error_content("Missing required parameter: path");
            }
        };
        let algorithm = args
            .get("algorithm")
            .and_then(|v| v.as_str())
            .unwrap_or("sha256");

        self.execute_cli_and_capture(&["abt", "checksum", path, "-a", algorithm])
    }

    fn tool_write(&self, args: &Value) -> Value {
        let source = match args.get("source").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return self.error_content("Missing required parameter: source"),
        };
        let target = match args.get("target").and_then(|v| v.as_str()) {
            Some(t) => t,
            None => return self.error_content("Missing required parameter: target"),
        };

        let mut cmd_parts: Vec<String> = vec![
            "abt".into(),
            "write".into(),
            "-i".into(),
            source.into(),
            "-o".into(),
            target.into(),
        ];

        if let Some(bs) = args.get("block_size").and_then(|v| v.as_str()) {
            cmd_parts.extend_from_slice(&["-b".into(), bs.into()]);
        }
        if args.get("verify").and_then(|v| v.as_bool()) == Some(false) {
            cmd_parts.push("--no-verify".into());
        }
        if let Some(algo) = args.get("hash_algorithm").and_then(|v| v.as_str()) {
            cmd_parts.extend_from_slice(&["--hash-algorithm".into(), algo.into()]);
        }
        if args.get("sparse").and_then(|v| v.as_bool()) == Some(true) {
            cmd_parts.push("--sparse".into());
        }
        if args.get("force").and_then(|v| v.as_bool()) == Some(true) {
            cmd_parts.push("--force".into());
        }
        if let Some(safety) = args.get("safety_level").and_then(|v| v.as_str()) {
            cmd_parts.extend_from_slice(&["--safety-level".into(), safety.into()]);
        }
        if args.get("dry_run").and_then(|v| v.as_bool()) == Some(true) {
            cmd_parts.push("--dry-run".into());
        }

        let parts_ref: Vec<&str> = cmd_parts.iter().map(|s| s.as_str()).collect();
        self.execute_cli_and_capture(&parts_ref)
    }

    fn tool_verify(&self, args: &Value) -> Value {
        let target = match args.get("target").and_then(|v| v.as_str()) {
            Some(t) => t,
            None => return self.error_content("Missing required parameter: target"),
        };

        let mut cmd_parts: Vec<String> = vec!["abt".into(), "verify".into(), "-o".into(), target.into()];

        if let Some(source) = args.get("source").and_then(|v| v.as_str()) {
            cmd_parts.extend_from_slice(&["-i".into(), source.into()]);
        }
        if let Some(hash) = args.get("expected_hash").and_then(|v| v.as_str()) {
            cmd_parts.extend_from_slice(&["--expected-hash".into(), hash.into()]);
        }
        if let Some(algo) = args.get("hash_algorithm").and_then(|v| v.as_str()) {
            cmd_parts.extend_from_slice(&["--hash-algorithm".into(), algo.into()]);
        }

        let parts_ref: Vec<&str> = cmd_parts.iter().map(|s| s.as_str()).collect();
        self.execute_cli_and_capture(&parts_ref)
    }

    fn tool_format(&self, args: &Value) -> Value {
        let device = match args.get("device").and_then(|v| v.as_str()) {
            Some(d) => d,
            None => return self.error_content("Missing required parameter: device"),
        };
        let filesystem = match args.get("filesystem").and_then(|v| v.as_str()) {
            Some(f) => f,
            None => return self.error_content("Missing required parameter: filesystem"),
        };

        let mut cmd_parts: Vec<String> = vec![
            "abt".into(),
            "format".into(),
            device.into(),
            "-f".into(),
            filesystem.into(),
        ];

        if let Some(label) = args.get("label").and_then(|v| v.as_str()) {
            cmd_parts.extend_from_slice(&["-l".into(), label.into()]);
        }
        if args.get("quick").and_then(|v| v.as_bool()) == Some(true) {
            cmd_parts.push("-q".into());
        }
        if args.get("force").and_then(|v| v.as_bool()) == Some(true) {
            cmd_parts.push("--force".into());
        }

        let parts_ref: Vec<&str> = cmd_parts.iter().map(|s| s.as_str()).collect();
        self.execute_cli_and_capture(&parts_ref)
    }

    // ── Helpers ────────────────────────────────────────────────────────

    /// Execute a CLI command as a subprocess and capture its output.
    /// In production, this spawns `abt` as a child process.
    /// Returns MCP-formatted content.
    fn execute_cli_and_capture(&self, args: &[&str]) -> Value {
        use std::process::Command;

        let exe = std::env::current_exe().unwrap_or_else(|_| "abt".into());
        let result = Command::new(&exe)
            .args(&args[1..]) // skip "abt"
            .arg("-o")
            .arg("json")
            .output();

        match result {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);

                if output.status.success() {
                    json!({
                        "content": [{
                            "type": "text",
                            "text": stdout.trim()
                        }]
                    })
                } else {
                    json!({
                        "content": [{
                            "type": "text",
                            "text": format!("Command failed (exit {}): {}\n{}", output.status, stdout.trim(), stderr.trim())
                        }],
                        "isError": true
                    })
                }
            }
            Err(e) => self.error_content(&format!("Failed to execute command: {}", e)),
        }
    }

    fn error_content(&self, msg: &str) -> Value {
        json!({
            "content": [{
                "type": "text",
                "text": msg
            }],
            "isError": true
        })
    }
}

// ─── Server entry point ────────────────────────────────────────────────────

/// Run the MCP server, reading JSON-RPC messages from stdin and writing
/// responses to stdout. Each message is a single line of JSON.
pub fn run_server(oneshot: bool) -> anyhow::Result<()> {
    let mut server = McpServer::new();
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    for line in stdin.lock().lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let request: JsonRpcRequest = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(e) => {
                let err_resp = JsonRpcResponse::error(
                    Value::Null,
                    -32700,
                    format!("Parse error: {}", e),
                );
                let json = serde_json::to_string(&err_resp)?;
                writeln!(stdout, "{}", json)?;
                stdout.flush()?;
                continue;
            }
        };

        if let Some(response) = server.handle_request(&request) {
            let json = serde_json::to_string(&response)?;
            writeln!(stdout, "{}", json)?;
            stdout.flush()?;
        }

        if oneshot {
            break;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_request(method: &str, params: Value) -> JsonRpcRequest {
        JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: method.into(),
            params,
        }
    }

    #[test]
    fn test_initialize() {
        let mut server = McpServer::new();
        let req = make_request("initialize", json!({}));
        let resp = server.handle_request(&req).unwrap();
        let result = resp.result.unwrap();
        assert_eq!(result["protocolVersion"], MCP_PROTOCOL_VERSION);
        assert_eq!(result["serverInfo"]["name"], SERVER_NAME);
        assert!(server.initialized);
    }

    #[test]
    fn test_ping() {
        let mut server = McpServer::new();
        let req = make_request("ping", json!({}));
        let resp = server.handle_request(&req).unwrap();
        assert!(resp.result.is_some());
        assert!(resp.error.is_none());
    }

    #[test]
    fn test_tools_list() {
        let mut server = McpServer::new();
        let req = make_request("tools/list", json!({}));
        let resp = server.handle_request(&req).unwrap();
        let result = resp.result.unwrap();
        let tools = result["tools"].as_array().unwrap();
        assert!(tools.len() >= 6);

        // Verify tool names
        let names: Vec<&str> = tools
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"abt_list_devices"));
        assert!(names.contains(&"abt_write"));
        assert!(names.contains(&"abt_verify"));
        assert!(names.contains(&"abt_checksum"));
        assert!(names.contains(&"abt_info"));
        assert!(names.contains(&"abt_format"));
    }

    #[test]
    fn test_unknown_method() {
        let mut server = McpServer::new();
        let req = make_request("nonexistent/method", json!({}));
        let resp = server.handle_request(&req).unwrap();
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32601);
    }

    #[test]
    fn test_notification_no_response() {
        let mut server = McpServer::new();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: None,
            method: "notifications/initialized".into(),
            params: json!({}),
        };
        assert!(server.handle_request(&req).is_none());
    }

    #[test]
    fn test_tools_call_missing_name() {
        let mut server = McpServer::new();
        let req = make_request("tools/call", json!({})); // no "name"
        let resp = server.handle_request(&req).unwrap();
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32602);
    }

    #[test]
    fn test_tools_call_unknown_tool() {
        let mut server = McpServer::new();
        let req = make_request(
            "tools/call",
            json!({ "name": "nonexistent_tool", "arguments": {} }),
        );
        let resp = server.handle_request(&req).unwrap();
        let result = resp.result.unwrap();
        assert_eq!(result["isError"], true);
    }

    #[test]
    fn test_tool_info_missing_path() {
        let server = McpServer::new();
        let result = server.tool_info(&json!({}));
        assert_eq!(result["isError"], true);
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("path"));
    }

    #[test]
    fn test_tool_write_missing_params() {
        let server = McpServer::new();
        let result = server.tool_write(&json!({}));
        assert_eq!(result["isError"], true);

        let result = server.tool_write(&json!({"source": "test.img"}));
        assert_eq!(result["isError"], true);
    }

    #[test]
    fn test_tool_definitions_schema() {
        let tools = tool_definitions();
        let tools = tools.as_array().unwrap();
        for tool in tools {
            assert!(tool.get("name").is_some());
            assert!(tool.get("description").is_some());
            assert!(tool.get("inputSchema").is_some());
            let schema = &tool["inputSchema"];
            assert_eq!(schema["type"], "object");
            assert!(schema.get("properties").is_some());
        }
    }
}
