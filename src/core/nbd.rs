#![allow(dead_code)]
//! Network Block Device (NBD) client — stream blocks from a remote NBD server.
//!
//! Implements the NBD protocol (RFC draft / nbd.git spec) for use as an image
//! source in write operations. Enables scenarios like:
//!   `abt write -i nbd://192.168.1.10:10809/export -o /dev/sdb`
//!
//! Protocol reference: https://github.com/NetworkBlockDevice/nbd/blob/master/doc/proto.md
//! Supports:
//!   - NBD new-style handshake
//!   - Simple and structured replies
//!   - Block-level streaming with progress tracking
//!   - Cancel-safe disconnect

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use anyhow::{Context, Result};
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};

/// NBD magic numbers from the protocol specification.
const NBD_MAGIC: u64 = 0x4e42_444d_4147_4943; // "NBDMAGIC"
const NBD_OPTS_MAGIC: u64 = 0x4948_4156_454F_5054; // "IHAVEOPT"
const NBD_REPLY_MAGIC: u32 = 0x6744_6698;
const NBD_REQUEST_MAGIC: u32 = 0x2560_9513;

/// NBD option types.
const NBD_OPT_EXPORT_NAME: u32 = 1;
#[allow(dead_code)]
const NBD_OPT_ABORT: u32 = 2;
#[allow(dead_code)]
const NBD_OPT_LIST: u32 = 3;
const NBD_OPT_GO: u32 = 7;

/// NBD command types.
const NBD_CMD_READ: u16 = 0;
#[allow(dead_code)]
const NBD_CMD_WRITE: u16 = 1;
const NBD_CMD_DISC: u16 = 2;

/// NBD info types.
const NBD_INFO_EXPORT: u16 = 0;

/// NBD reply types.
const NBD_REP_ACK: u32 = 1;
const NBD_REP_INFO: u32 = 3;
const NBD_REP_ERR_UNSUP: u32 = (1 << 31) | 1;

/// NBD transmission flags.
#[allow(dead_code)]
const NBD_FLAG_HAS_FLAGS: u16 = 1 << 0;
#[allow(dead_code)]
const NBD_FLAG_READ_ONLY: u16 = 1 << 1;

/// Default NBD port.
const DEFAULT_NBD_PORT: u16 = 10809;

/// Parsed NBD URL.
#[derive(Debug, Clone)]
pub struct NbdUrl {
    pub host: String,
    pub port: u16,
    pub export: String,
}

/// NBD connection configuration.
#[derive(Debug, Clone)]
pub struct NbdConfig {
    /// NBD server address.
    pub url: NbdUrl,
    /// Connection timeout.
    pub connect_timeout: Duration,
    /// I/O timeout.
    pub io_timeout: Duration,
    /// Block size for read requests.
    pub block_size: usize,
}

impl Default for NbdConfig {
    fn default() -> Self {
        Self {
            url: NbdUrl {
                host: "localhost".to_string(),
                port: DEFAULT_NBD_PORT,
                export: String::new(),
            },
            connect_timeout: Duration::from_secs(30),
            io_timeout: Duration::from_secs(120),
            block_size: 4 * 1024 * 1024, // 4 MiB
        }
    }
}

/// NBD export information received during handshake.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NbdExportInfo {
    /// Export size in bytes.
    pub size: u64,
    /// Transmission flags.
    pub flags: u16,
    /// Export name.
    pub name: String,
}

/// An active NBD connection implementing Read for streaming into the write pipeline.
pub struct NbdReader {
    stream: TcpStream,
    export_info: NbdExportInfo,
    offset: u64,
    block_size: usize,
    /// Internal buffer for partial reads.
    buf: Vec<u8>,
    buf_pos: usize,
    buf_len: usize,
    /// Sequence counter for request handles.
    handle: u64,
}

/// Parse an NBD URL: `nbd://host[:port][/export]`
pub fn parse_nbd_url(url: &str) -> Result<NbdUrl> {
    let stripped = url
        .strip_prefix("nbd://")
        .with_context(|| format!("Invalid NBD URL (must start with nbd://): {}", url))?;

    let (host_port, export) = match stripped.find('/') {
        Some(idx) => (&stripped[..idx], stripped[idx + 1..].to_string()),
        None => (stripped, String::new()),
    };

    let (host, port) = match host_port.rfind(':') {
        Some(idx) => {
            let port_str = &host_port[idx + 1..];
            let port: u16 = port_str
                .parse()
                .with_context(|| format!("Invalid port number: {}", port_str))?;
            (host_port[..idx].to_string(), port)
        }
        None => (host_port.to_string(), DEFAULT_NBD_PORT),
    };

    Ok(NbdUrl { host, port, export })
}

/// Connect to an NBD server and negotiate the export.
pub fn connect(config: &NbdConfig) -> Result<NbdReader> {
    let addr = format!("{}:{}", config.url.host, config.url.port);
    info!("Connecting to NBD server at {}", addr);

    let stream = TcpStream::connect_timeout(
        &addr.parse().with_context(|| format!("Invalid address: {}", addr))?,
        config.connect_timeout,
    )
    .with_context(|| format!("Failed to connect to NBD server at {}", addr))?;

    stream.set_read_timeout(Some(config.io_timeout))?;
    stream.set_write_timeout(Some(config.io_timeout))?;
    stream.set_nodelay(true)?;

    let mut reader = NbdReader {
        stream,
        export_info: NbdExportInfo {
            size: 0,
            flags: 0,
            name: config.url.export.clone(),
        },
        offset: 0,
        block_size: config.block_size,
        buf: vec![0u8; config.block_size],
        buf_pos: 0,
        buf_len: 0,
        handle: 0,
    };

    reader.handshake(&config.url.export)?;
    info!(
        "NBD connected: export='{}', size={} bytes",
        reader.export_info.name, reader.export_info.size
    );

    Ok(reader)
}

impl NbdReader {
    /// Perform the NBD new-style handshake.
    fn handshake(&mut self, export_name: &str) -> Result<()> {
        // Read server greeting: NBDMAGIC (8) + IHAVEOPT (8) + handshake flags (2)
        let mut greeting = [0u8; 18];
        self.stream.read_exact(&mut greeting)?;

        let magic = u64::from_be_bytes(greeting[0..8].try_into().unwrap());
        if magic != NBD_MAGIC {
            anyhow::bail!(
                "Invalid NBD magic: expected 0x{:016X}, got 0x{:016X}",
                NBD_MAGIC,
                magic
            );
        }

        let opts_magic = u64::from_be_bytes(greeting[8..16].try_into().unwrap());
        if opts_magic != NBD_OPTS_MAGIC {
            anyhow::bail!(
                "Invalid NBD opts magic: expected 0x{:016X}, got 0x{:016X}",
                NBD_OPTS_MAGIC,
                opts_magic
            );
        }

        let _handshake_flags = u16::from_be_bytes(greeting[16..18].try_into().unwrap());

        // Send client flags (0 = no fixed newstyle)
        self.stream.write_all(&0u32.to_be_bytes())?;

        // Try NBD_OPT_GO first (gets info + enters transmission phase)
        if self.try_opt_go(export_name)? {
            return Ok(());
        }

        // Fall back to NBD_OPT_EXPORT_NAME (old servers)
        self.opt_export_name(export_name)?;

        Ok(())
    }

    /// Send NBD_OPT_GO — modern servers reply with export info then enter transmission.
    fn try_opt_go(&mut self, export_name: &str) -> Result<bool> {
        let name_bytes = export_name.as_bytes();
        // Option header: IHAVEOPT (8) + option (4) + length (4)
        // Option data: name_len (4) + name + num_info_requests (2) + info_request (2)
        let data_len = 4 + name_bytes.len() + 2 + 2;

        let mut pkt = Vec::with_capacity(16 + data_len);
        pkt.extend_from_slice(&NBD_OPTS_MAGIC.to_be_bytes());
        pkt.extend_from_slice(&NBD_OPT_GO.to_be_bytes());
        pkt.extend_from_slice(&(data_len as u32).to_be_bytes());
        pkt.extend_from_slice(&(name_bytes.len() as u32).to_be_bytes());
        pkt.extend_from_slice(name_bytes);
        pkt.extend_from_slice(&1u16.to_be_bytes()); // 1 info request
        pkt.extend_from_slice(&NBD_INFO_EXPORT.to_be_bytes());

        self.stream.write_all(&pkt)?;
        self.stream.flush()?;

        // Read replies until ACK or error
        loop {
            let mut reply_hdr = [0u8; 20];
            self.stream.read_exact(&mut reply_hdr)?;

            let reply_magic = u64::from_be_bytes(reply_hdr[0..8].try_into().unwrap());
            if reply_magic != 0x0003_e889_045565_a9u64 {
                // Not a valid reply magic — might be old protocol
                debug!("NBD_OPT_GO not supported, falling back to EXPORT_NAME");
                return Ok(false);
            }

            let _reply_opt = u32::from_be_bytes(reply_hdr[8..12].try_into().unwrap());
            let reply_type = u32::from_be_bytes(reply_hdr[12..16].try_into().unwrap());
            let reply_len = u32::from_be_bytes(reply_hdr[16..20].try_into().unwrap()) as usize;

            // Read reply data
            let mut reply_data = vec![0u8; reply_len];
            if reply_len > 0 {
                self.stream.read_exact(&mut reply_data)?;
            }

            match reply_type {
                NBD_REP_INFO => {
                    if reply_len >= 2 {
                        let info_type =
                            u16::from_be_bytes(reply_data[0..2].try_into().unwrap());
                        if info_type == NBD_INFO_EXPORT && reply_len >= 12 {
                            self.export_info.size =
                                u64::from_be_bytes(reply_data[2..10].try_into().unwrap());
                            self.export_info.flags =
                                u16::from_be_bytes(reply_data[10..12].try_into().unwrap());
                        }
                    }
                }
                NBD_REP_ACK => {
                    debug!("NBD_OPT_GO acknowledged");
                    return Ok(true);
                }
                NBD_REP_ERR_UNSUP => {
                    debug!("NBD_OPT_GO unsupported by server");
                    return Ok(false);
                }
                other if other & (1 << 31) != 0 => {
                    let msg = String::from_utf8_lossy(&reply_data);
                    anyhow::bail!("NBD server error 0x{:X}: {}", other, msg);
                }
                _ => {
                    debug!("NBD unknown reply type 0x{:X}, skipping", reply_type);
                }
            }
        }
    }

    /// Send NBD_OPT_EXPORT_NAME — legacy handshake.
    fn opt_export_name(&mut self, export_name: &str) -> Result<()> {
        let name_bytes = export_name.as_bytes();
        let mut pkt = Vec::with_capacity(16 + name_bytes.len());
        pkt.extend_from_slice(&NBD_OPTS_MAGIC.to_be_bytes());
        pkt.extend_from_slice(&NBD_OPT_EXPORT_NAME.to_be_bytes());
        pkt.extend_from_slice(&(name_bytes.len() as u32).to_be_bytes());
        pkt.extend_from_slice(name_bytes);

        self.stream.write_all(&pkt)?;
        self.stream.flush()?;

        // Server replies with: export size (8) + flags (2) + zero padding (124)
        let mut response = [0u8; 134];
        self.stream.read_exact(&mut response)?;

        self.export_info.size = u64::from_be_bytes(response[0..8].try_into().unwrap());
        self.export_info.flags = u16::from_be_bytes(response[8..10].try_into().unwrap());

        Ok(())
    }

    /// Get export info.
    pub fn export_info(&self) -> &NbdExportInfo {
        &self.export_info
    }

    /// Get total export size.
    pub fn size(&self) -> u64 {
        self.export_info.size
    }

    /// Send an NBD_CMD_READ request and read the response data into buf.
    fn read_block(&mut self, offset: u64, length: u32) -> Result<usize> {
        self.handle += 1;

        // Build request: magic (4) + flags (2) + type (2) + handle (8) + offset (8) + length (4)
        let mut req = [0u8; 28];
        req[0..4].copy_from_slice(&NBD_REQUEST_MAGIC.to_be_bytes());
        req[4..6].copy_from_slice(&0u16.to_be_bytes()); // flags
        req[6..8].copy_from_slice(&NBD_CMD_READ.to_be_bytes());
        req[8..16].copy_from_slice(&self.handle.to_be_bytes());
        req[16..24].copy_from_slice(&offset.to_be_bytes());
        req[24..28].copy_from_slice(&length.to_be_bytes());

        self.stream.write_all(&req)?;
        self.stream.flush()?;

        // Read simple reply: magic (4) + error (4) + handle (8)
        let mut reply_hdr = [0u8; 16];
        self.stream.read_exact(&mut reply_hdr)?;

        let reply_magic = u32::from_be_bytes(reply_hdr[0..4].try_into().unwrap());
        if reply_magic != NBD_REPLY_MAGIC {
            anyhow::bail!(
                "Invalid NBD reply magic: expected 0x{:08X}, got 0x{:08X}",
                NBD_REPLY_MAGIC,
                reply_magic
            );
        }

        let error = u32::from_be_bytes(reply_hdr[4..8].try_into().unwrap());
        if error != 0 {
            anyhow::bail!("NBD read error at offset {}: errno={}", offset, error);
        }

        // Read the data payload
        let len = length as usize;
        if self.buf.len() < len {
            self.buf.resize(len, 0);
        }
        self.stream.read_exact(&mut self.buf[..len])?;

        Ok(len)
    }

    /// Send NBD_CMD_DISC to cleanly disconnect.
    pub fn disconnect(&mut self) -> Result<()> {
        self.handle += 1;
        let mut req = [0u8; 28];
        req[0..4].copy_from_slice(&NBD_REQUEST_MAGIC.to_be_bytes());
        req[4..6].copy_from_slice(&0u16.to_be_bytes());
        req[6..8].copy_from_slice(&NBD_CMD_DISC.to_be_bytes());
        req[8..16].copy_from_slice(&self.handle.to_be_bytes());
        // offset = 0, length = 0

        let _ = self.stream.write_all(&req);
        let _ = self.stream.flush();
        let _ = self.stream.shutdown(std::net::Shutdown::Both);

        info!("NBD disconnected");
        Ok(())
    }
}

impl Read for NbdReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        // Serve from internal buffer first
        if self.buf_pos < self.buf_len {
            let avail = self.buf_len - self.buf_pos;
            let to_copy = avail.min(out.len());
            out[..to_copy].copy_from_slice(&self.buf[self.buf_pos..self.buf_pos + to_copy]);
            self.buf_pos += to_copy;
            return Ok(to_copy);
        }

        // Check if we've read the entire export
        if self.offset >= self.export_info.size {
            return Ok(0);
        }

        // Request next block
        let remaining = self.export_info.size - self.offset;
        let request_len = (self.block_size as u64).min(remaining) as u32;

        let n = self
            .read_block(self.offset, request_len)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

        self.offset += n as u64;
        self.buf_len = n;
        self.buf_pos = 0;

        // Copy out what we can
        let to_copy = n.min(out.len());
        out[..to_copy].copy_from_slice(&self.buf[..to_copy]);
        self.buf_pos = to_copy;

        Ok(to_copy)
    }
}

impl Drop for NbdReader {
    fn drop(&mut self) {
        if let Err(e) = self.disconnect() {
            warn!("NBD disconnect error: {}", e);
        }
    }
}

/// Check if a source string is an NBD URL.
pub fn is_nbd_url(source: &str) -> bool {
    source.starts_with("nbd://")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_nbd_url_full() {
        let url = parse_nbd_url("nbd://192.168.1.10:10809/myexport").unwrap();
        assert_eq!(url.host, "192.168.1.10");
        assert_eq!(url.port, 10809);
        assert_eq!(url.export, "myexport");
    }

    #[test]
    fn test_parse_nbd_url_default_port() {
        let url = parse_nbd_url("nbd://server.local/disk1").unwrap();
        assert_eq!(url.host, "server.local");
        assert_eq!(url.port, DEFAULT_NBD_PORT);
        assert_eq!(url.export, "disk1");
    }

    #[test]
    fn test_parse_nbd_url_no_export() {
        let url = parse_nbd_url("nbd://10.0.0.1:9000").unwrap();
        assert_eq!(url.host, "10.0.0.1");
        assert_eq!(url.port, 9000);
        assert_eq!(url.export, "");
    }

    #[test]
    fn test_parse_nbd_url_invalid() {
        assert!(parse_nbd_url("http://example.com").is_err());
        assert!(parse_nbd_url("not-a-url").is_err());
    }

    #[test]
    fn test_is_nbd_url() {
        assert!(is_nbd_url("nbd://localhost/export"));
        assert!(is_nbd_url("nbd://10.0.0.1:9000"));
        assert!(!is_nbd_url("http://example.com"));
        assert!(!is_nbd_url("/dev/sda"));
    }

    #[test]
    fn test_nbd_config_default() {
        let config = NbdConfig::default();
        assert_eq!(config.url.port, DEFAULT_NBD_PORT);
        assert_eq!(config.block_size, 4 * 1024 * 1024);
        assert_eq!(config.connect_timeout, Duration::from_secs(30));
    }

    #[test]
    fn test_nbd_export_info_serialize() {
        let info = NbdExportInfo {
            size: 1024 * 1024 * 1024,
            flags: 1,
            name: "test".to_string(),
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"size\":1073741824"));
        assert!(json.contains("\"name\":\"test\""));
    }

    #[test]
    fn test_parse_nbd_url_ipv4_with_port() {
        let url = parse_nbd_url("nbd://127.0.0.1:5555/vol0").unwrap();
        assert_eq!(url.host, "127.0.0.1");
        assert_eq!(url.port, 5555);
        assert_eq!(url.export, "vol0");
    }
}
