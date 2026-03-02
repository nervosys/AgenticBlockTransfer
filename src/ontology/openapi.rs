//! OpenAPI 3.1 schema generator for AgenticBlockTransfer.
//!
//! Generates a complete OpenAPI specification that describes abt's capabilities
//! as if they were exposed via a REST/HTTP API. Useful for wrapping abt in a
//! web service, generating client SDKs, or feeding into AI tool-use schemas.

use serde_json::{json, Value};

/// Generate a complete OpenAPI 3.1 specification for abt.
pub fn generate_openapi_spec() -> Value {
    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "AgenticBlockTransfer (abt) API",
            "version": env!("CARGO_PKG_VERSION"),
            "description": "REST API specification for AgenticBlockTransfer — an agentic-first \
                block transfer tool for writing disk images to devices. This specification \
                describes all capabilities as HTTP endpoints for use in web service wrappers, \
                SDK generators, and AI tool-use schemas.",
            "license": {
                "name": "MIT OR Apache-2.0",
                "identifier": "MIT OR Apache-2.0"
            },
            "contact": {
                "name": "nervosys",
                "url": "https://github.com/nervosys/AgenticBlockTransfer"
            }
        },
        "servers": [
            {
                "url": "http://localhost:8080",
                "description": "Local abt HTTP wrapper"
            }
        ],
        "tags": [
            { "name": "data-transfer", "description": "Image write operations" },
            { "name": "verification", "description": "Hash and data verification" },
            { "name": "devices", "description": "Device enumeration and management" },
            { "name": "information", "description": "Image and device inspection" },
            { "name": "formatting", "description": "Device formatting" },
            { "name": "meta", "description": "Ontology and schema endpoints" }
        ],
        "paths": build_paths(),
        "components": {
            "schemas": build_schemas(),
            "parameters": build_parameters(),
            "responses": build_responses()
        }
    })
}

fn build_paths() -> Value {
    json!({
        "/write": {
            "post": {
                "operationId": "writeImage",
                "summary": "Write an image to a target device",
                "description": "Write a disk image file or URL to a target block device. \
                    Supports automatic format detection, decompression (gz/bz2/xz/zstd/zip), \
                    sparse writes, direct I/O, and optional post-write verification.",
                "tags": ["data-transfer"],
                "requestBody": {
                    "required": true,
                    "content": {
                        "application/json": {
                            "schema": { "$ref": "#/components/schemas/WriteRequest" }
                        }
                    }
                },
                "responses": {
                    "200": {
                        "description": "Write completed successfully",
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/WriteResult" }
                            }
                        }
                    },
                    "400": { "$ref": "#/components/responses/SafetyCheckFailed" },
                    "403": { "$ref": "#/components/responses/PermissionDenied" },
                    "404": { "$ref": "#/components/responses/SourceNotFound" },
                    "409": { "$ref": "#/components/responses/DeviceChanged" },
                    "422": { "$ref": "#/components/responses/ImageTooLarge" },
                    "500": { "$ref": "#/components/responses/GeneralError" }
                }
            }
        },
        "/verify": {
            "post": {
                "operationId": "verifyDevice",
                "summary": "Verify written data against source or hash",
                "description": "Read back written data and compare against the original source \
                    image or an expected hash value.",
                "tags": ["verification"],
                "requestBody": {
                    "required": true,
                    "content": {
                        "application/json": {
                            "schema": { "$ref": "#/components/schemas/VerifyRequest" }
                        }
                    }
                },
                "responses": {
                    "200": {
                        "description": "Verification result",
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/VerifyResult" }
                            }
                        }
                    },
                    "500": { "$ref": "#/components/responses/GeneralError" }
                }
            }
        },
        "/devices": {
            "get": {
                "operationId": "listDevices",
                "summary": "List available block devices",
                "description": "Enumerate storage devices on the system. Returns device paths, \
                    sizes, types, and device fingerprint tokens for TOCTOU-safe writes.",
                "tags": ["devices"],
                "parameters": [
                    { "$ref": "#/components/parameters/ShowAll" },
                    { "$ref": "#/components/parameters/DeviceTypeFilter" },
                    { "$ref": "#/components/parameters/RemovableOnly" }
                ],
                "responses": {
                    "200": {
                        "description": "Device list",
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/DeviceList" }
                            }
                        }
                    }
                }
            }
        },
        "/info/{path}": {
            "get": {
                "operationId": "getInfo",
                "summary": "Inspect a device or image file",
                "description": "Show detailed information about a device or image file, \
                    including format detection, partition tables, ISO 9660 metadata, \
                    QCOW2/VHD/VMDK/WIM headers, and more.",
                "tags": ["information"],
                "parameters": [
                    {
                        "name": "path",
                        "in": "path",
                        "required": true,
                        "schema": { "type": "string" },
                        "description": "Device path or image file path"
                    }
                ],
                "responses": {
                    "200": {
                        "description": "Device or image information",
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/InfoResult" }
                            }
                        }
                    }
                }
            }
        },
        "/checksum": {
            "post": {
                "operationId": "computeChecksum",
                "summary": "Compute checksums of a file or device",
                "description": "Compute one or more hash digests (SHA-256, SHA-512, MD5, \
                    BLAKE3, CRC32, SHA-1) of a file or device.",
                "tags": ["verification"],
                "requestBody": {
                    "required": true,
                    "content": {
                        "application/json": {
                            "schema": { "$ref": "#/components/schemas/ChecksumRequest" }
                        }
                    }
                },
                "responses": {
                    "200": {
                        "description": "Checksum results",
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/ChecksumResult" }
                            }
                        }
                    }
                }
            }
        },
        "/format": {
            "post": {
                "operationId": "formatDevice",
                "summary": "Format a device with a filesystem",
                "description": "Format a block device with the specified filesystem type. \
                    Supports FAT16, FAT32, exFAT, NTFS, ext2/3/4, XFS, Btrfs.",
                "tags": ["formatting"],
                "requestBody": {
                    "required": true,
                    "content": {
                        "application/json": {
                            "schema": { "$ref": "#/components/schemas/FormatRequest" }
                        }
                    }
                },
                "responses": {
                    "200": {
                        "description": "Format completed",
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/OperationResult" }
                            }
                        }
                    },
                    "403": { "$ref": "#/components/responses/PermissionDenied" }
                }
            }
        },
        "/ontology": {
            "get": {
                "operationId": "getOntology",
                "summary": "Export AI-discoverable capability ontology",
                "description": "Returns the complete capability schema in JSON-LD, JSON, or YAML \
                    format for AI agent integration.",
                "tags": ["meta"],
                "parameters": [
                    {
                        "name": "format",
                        "in": "query",
                        "schema": {
                            "type": "string",
                            "enum": ["json-ld", "json", "yaml"],
                            "default": "json-ld"
                        },
                        "description": "Output format"
                    },
                    {
                        "name": "full",
                        "in": "query",
                        "schema": { "type": "boolean", "default": false },
                        "description": "Include full parameter schemas"
                    }
                ],
                "responses": {
                    "200": {
                        "description": "Ontology schema",
                        "content": {
                            "application/json": {},
                            "application/ld+json": {},
                            "application/x-yaml": {}
                        }
                    }
                }
            }
        },
        "/erase": {
            "post": {
                "operationId": "secureErase",
                "summary": "Securely erase a device",
                "description": "Perform a secure erase on a block device using the most \
                    appropriate method: ATA SECURITY ERASE UNIT, NVMe sanitize, \
                    blkdiscard, or zero-fill fallback.",
                "tags": ["devices"],
                "requestBody": {
                    "required": true,
                    "content": {
                        "application/json": {
                            "schema": { "$ref": "#/components/schemas/EraseRequest" }
                        }
                    }
                },
                "responses": {
                    "200": {
                        "description": "Erase completed",
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/OperationResult" }
                            }
                        }
                    },
                    "403": { "$ref": "#/components/responses/PermissionDenied" }
                }
            }
        },
        "/clone": {
            "post": {
                "operationId": "cloneDevice",
                "summary": "Clone one device to another",
                "description": "Perform a block-level clone from a source device to a target \
                    device with progress tracking and optional verification.",
                "tags": ["data-transfer"],
                "requestBody": {
                    "required": true,
                    "content": {
                        "application/json": {
                            "schema": { "$ref": "#/components/schemas/CloneRequest" }
                        }
                    }
                },
                "responses": {
                    "200": {
                        "description": "Clone completed",
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/WriteResult" }
                            }
                        }
                    },
                    "400": { "$ref": "#/components/responses/SafetyCheckFailed" },
                    "403": { "$ref": "#/components/responses/PermissionDenied" }
                }
            }
        }
    })
}

fn build_schemas() -> Value {
    json!({
        "WriteRequest": {
            "type": "object",
            "required": ["source", "target"],
            "properties": {
                "source": {
                    "type": "string",
                    "description": "Source image file path or URL"
                },
                "target": {
                    "type": "string",
                    "description": "Target device path (e.g., /dev/sdb, \\\\.\\PhysicalDrive1)"
                },
                "block_size": {
                    "type": "string",
                    "default": "4M",
                    "description": "I/O block size (e.g., 512, 4K, 1M, 4M)"
                },
                "verify": {
                    "type": "boolean",
                    "default": true,
                    "description": "Verify by reading back after write"
                },
                "hash_algorithm": {
                    "type": "string",
                    "enum": ["md5", "sha1", "sha256", "sha512", "blake3", "crc32"],
                    "default": "sha256"
                },
                "expected_hash": {
                    "type": "string",
                    "description": "Expected hash for pre-write integrity check"
                },
                "direct_io": {
                    "type": "boolean",
                    "default": true,
                    "description": "Use O_DIRECT / FILE_FLAG_NO_BUFFERING"
                },
                "sparse": {
                    "type": "boolean",
                    "default": false,
                    "description": "Skip all-zero blocks"
                },
                "safety_level": {
                    "type": "string",
                    "enum": ["low", "medium", "high"],
                    "default": "low"
                },
                "confirm_token": {
                    "type": "string",
                    "description": "Device fingerprint token from /devices (TOCTOU prevention)"
                },
                "dry_run": {
                    "type": "boolean",
                    "default": false,
                    "description": "Run safety checks only, do not write"
                }
            }
        },
        "VerifyRequest": {
            "type": "object",
            "required": ["target"],
            "properties": {
                "source": {
                    "type": "string",
                    "description": "Source image path for comparison"
                },
                "target": {
                    "type": "string",
                    "description": "Target device or file to verify"
                },
                "expected_hash": {
                    "type": "string",
                    "description": "Expected hash (algorithm:hex format)"
                },
                "hash_algorithm": {
                    "type": "string",
                    "enum": ["md5", "sha1", "sha256", "sha512", "blake3", "crc32"],
                    "default": "sha256"
                }
            }
        },
        "ChecksumRequest": {
            "type": "object",
            "required": ["path"],
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File or device path to hash"
                },
                "algorithms": {
                    "type": "array",
                    "items": {
                        "type": "string",
                        "enum": ["md5", "sha1", "sha256", "sha512", "blake3", "crc32"]
                    },
                    "default": ["sha256"]
                }
            }
        },
        "FormatRequest": {
            "type": "object",
            "required": ["device", "filesystem"],
            "properties": {
                "device": {
                    "type": "string",
                    "description": "Device path to format"
                },
                "filesystem": {
                    "type": "string",
                    "enum": ["fat16", "fat32", "exfat", "ntfs", "ext2", "ext3", "ext4", "xfs", "btrfs"],
                    "description": "Filesystem type"
                },
                "label": {
                    "type": "string",
                    "description": "Volume label"
                },
                "quick": {
                    "type": "boolean",
                    "default": false,
                    "description": "Quick format"
                }
            }
        },
        "EraseRequest": {
            "type": "object",
            "required": ["device"],
            "properties": {
                "device": {
                    "type": "string",
                    "description": "Device path to erase"
                },
                "method": {
                    "type": "string",
                    "enum": ["auto", "zero", "random", "ata-secure-erase", "nvme-sanitize", "discard"],
                    "default": "auto",
                    "description": "Erase method"
                },
                "passes": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 35,
                    "default": 1,
                    "description": "Number of overwrite passes"
                }
            }
        },
        "CloneRequest": {
            "type": "object",
            "required": ["source", "target"],
            "properties": {
                "source": {
                    "type": "string",
                    "description": "Source device path"
                },
                "target": {
                    "type": "string",
                    "description": "Target device path"
                },
                "block_size": {
                    "type": "string",
                    "default": "4M"
                },
                "verify": {
                    "type": "boolean",
                    "default": true
                },
                "sparse": {
                    "type": "boolean",
                    "default": false,
                    "description": "Skip zero blocks during clone"
                }
            }
        },
        "WriteResult": {
            "type": "object",
            "properties": {
                "success": { "type": "boolean" },
                "bytes_written": { "type": "integer", "format": "int64" },
                "bytes_sparse_skipped": { "type": "integer", "format": "int64" },
                "hash": { "type": "string" },
                "duration_seconds": { "type": "number" },
                "speed_mbps": { "type": "number" },
                "verified": { "type": "boolean" }
            }
        },
        "VerifyResult": {
            "type": "object",
            "properties": {
                "matches": { "type": "boolean" },
                "hash": { "type": "string" },
                "mismatch_offset": {
                    "type": "integer",
                    "format": "int64",
                    "description": "Byte offset of first mismatch (if any)"
                }
            }
        },
        "ChecksumResult": {
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "checksums": {
                    "type": "object",
                    "additionalProperties": { "type": "string" },
                    "description": "Map of algorithm name to hex digest"
                }
            }
        },
        "DeviceList": {
            "type": "object",
            "properties": {
                "devices": {
                    "type": "array",
                    "items": { "$ref": "#/components/schemas/DeviceInfo" }
                }
            }
        },
        "DeviceInfo": {
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "name": { "type": "string" },
                "size": { "type": "integer", "format": "int64" },
                "size_human": { "type": "string" },
                "device_type": {
                    "type": "string",
                    "enum": ["usb", "sd", "nvme", "sata", "scsi", "mmc", "emmc", "spi", "i2c_eeprom", "virtual", "network", "unknown"]
                },
                "removable": { "type": "boolean" },
                "read_only": { "type": "boolean" },
                "model": { "type": "string" },
                "vendor": { "type": "string" },
                "serial": { "type": "string" },
                "is_system_drive": { "type": "boolean" },
                "confirm_token": {
                    "type": "string",
                    "description": "Device fingerprint token for TOCTOU-safe write operations"
                }
            }
        },
        "InfoResult": {
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "type": {
                    "type": "string",
                    "enum": ["device", "image"]
                },
                "format": { "type": "string" },
                "size": { "type": "integer", "format": "int64" },
                "partitions": {
                    "type": "array",
                    "items": { "$ref": "#/components/schemas/PartitionInfo" }
                },
                "metadata": {
                    "type": "object",
                    "description": "Format-specific metadata (ISO 9660, QCOW2, VHD, VMDK, WIM)"
                }
            }
        },
        "PartitionInfo": {
            "type": "object",
            "properties": {
                "index": { "type": "integer" },
                "type_name": { "type": "string" },
                "label": { "type": "string" },
                "size": { "type": "integer", "format": "int64" },
                "start_lba": { "type": "integer", "format": "int64" },
                "filesystem": { "type": "string" },
                "bootable": { "type": "boolean" }
            }
        },
        "OperationResult": {
            "type": "object",
            "properties": {
                "success": { "type": "boolean" },
                "message": { "type": "string" }
            }
        }
    })
}

fn build_parameters() -> Value {
    json!({
        "ShowAll": {
            "name": "all",
            "in": "query",
            "schema": { "type": "boolean", "default": false },
            "description": "Show all devices including system drives"
        },
        "DeviceTypeFilter": {
            "name": "type",
            "in": "query",
            "schema": {
                "type": "string",
                "enum": ["usb", "sd", "nvme", "sata", "scsi", "mmc", "emmc"]
            },
            "description": "Filter by device type"
        },
        "RemovableOnly": {
            "name": "removable",
            "in": "query",
            "schema": { "type": "boolean", "default": false },
            "description": "Show only removable devices"
        }
    })
}

fn build_responses() -> Value {
    json!({
        "GeneralError": {
            "description": "General error (exit code 1)",
            "content": {
                "application/json": {
                    "schema": {
                        "type": "object",
                        "properties": {
                            "error": { "type": "string" },
                            "exit_code": { "type": "integer" }
                        }
                    }
                }
            }
        },
        "SafetyCheckFailed": {
            "description": "Safety check failed — write blocked by pre-flight analysis (exit code 2)"
        },
        "PermissionDenied": {
            "description": "Insufficient privileges — run as root/administrator (exit code 4)"
        },
        "SourceNotFound": {
            "description": "Source image not found or unreadable (exit code 5)"
        },
        "DeviceChanged": {
            "description": "Device fingerprint changed since enumeration — TOCTOU detected (exit code 8)"
        },
        "ImageTooLarge": {
            "description": "Image is larger than the target device (exit code 7)"
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openapi_spec_structure() {
        let spec = generate_openapi_spec();
        assert_eq!(spec["openapi"], "3.1.0");
        assert!(spec["info"]["title"].as_str().unwrap().contains("abt"));
    }

    #[test]
    fn test_openapi_has_all_paths() {
        let spec = generate_openapi_spec();
        let paths = &spec["paths"];
        assert!(paths["/write"].is_object());
        assert!(paths["/verify"].is_object());
        assert!(paths["/devices"].is_object());
        assert!(paths["/info/{path}"].is_object());
        assert!(paths["/checksum"].is_object());
        assert!(paths["/format"].is_object());
        assert!(paths["/ontology"].is_object());
        assert!(paths["/erase"].is_object());
        assert!(paths["/clone"].is_object());
    }

    #[test]
    fn test_openapi_schemas_defined() {
        let spec = generate_openapi_spec();
        let schemas = &spec["components"]["schemas"];
        assert!(schemas["WriteRequest"].is_object());
        assert!(schemas["WriteResult"].is_object());
        assert!(schemas["DeviceInfo"].is_object());
        assert!(schemas["VerifyRequest"].is_object());
        assert!(schemas["EraseRequest"].is_object());
        assert!(schemas["CloneRequest"].is_object());
    }

    #[test]
    fn test_openapi_write_request_has_required_fields() {
        let spec = generate_openapi_spec();
        let write_req = &spec["components"]["schemas"]["WriteRequest"];
        let required = write_req["required"].as_array().unwrap();
        assert!(required.contains(&json!("source")));
        assert!(required.contains(&json!("target")));
    }

    #[test]
    fn test_openapi_device_info_has_confirm_token() {
        let spec = generate_openapi_spec();
        let device_info = &spec["components"]["schemas"]["DeviceInfo"];
        assert!(device_info["properties"]["confirm_token"].is_object());
    }

    #[test]
    fn test_openapi_error_responses() {
        let spec = generate_openapi_spec();
        let responses = &spec["components"]["responses"];
        assert!(responses["GeneralError"].is_object());
        assert!(responses["SafetyCheckFailed"].is_object());
        assert!(responses["PermissionDenied"].is_object());
        assert!(responses["DeviceChanged"].is_object());
    }

    #[test]
    fn test_openapi_serializable() {
        let spec = generate_openapi_spec();
        let json_str = serde_json::to_string_pretty(&spec).unwrap();
        assert!(json_str.len() > 1000);
    }

    #[test]
    fn test_openapi_yaml_serializable() {
        let spec = generate_openapi_spec();
        let yaml_str = serde_yaml::to_string(&spec).unwrap();
        assert!(yaml_str.contains("openapi"));
        assert!(yaml_str.contains("writeImage"));
    }
}
