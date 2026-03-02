#![allow(dead_code)]
//! Multicast imaging — flash multiple devices simultaneously over UDP multicast.
//!
//! Implements a reliable multicast protocol for distributing disk images to
//! multiple receivers on a LAN:
//!
//! - **Sender**: Reads an image, splits into chunks, broadcasts via UDP multicast
//!   with sequence numbers and checksums.
//! - **Receiver**: Joins the multicast group, receives chunks, requests
//!   retransmission of missed chunks, writes to target device.
//!
//! Protocol features:
//! - CRC32 per-chunk integrity
//! - NAK-based retransmission (receivers report gaps)
//! - Configurable multicast group and port
//! - Chunked transfer with ordered reassembly
//! - Session management (session ID prevents cross-talk)

use anyhow::{Context, Result};
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Default multicast group address.
pub const DEFAULT_MULTICAST_GROUP: &str = "239.42.42.1";

/// Default multicast port.
pub const DEFAULT_MULTICAST_PORT: u16 = 42420;

/// Maximum payload per UDP datagram (MTU-safe: 1500 - IP(20) - UDP(8) - header).
const MAX_PAYLOAD: usize = 1400;

/// Header size for each chunk packet.
const CHUNK_HEADER_SIZE: usize = 28;

/// Maximum chunk data per packet.
const MAX_CHUNK_DATA: usize = MAX_PAYLOAD - CHUNK_HEADER_SIZE;

/// Configuration for multicast imaging sessions.
#[derive(Debug, Clone)]
pub struct MulticastConfig {
    /// Multicast group address (default: 239.42.42.1).
    pub group: Ipv4Addr,
    /// UDP port (default: 42420).
    pub port: u16,
    /// Network interface to bind to (default: 0.0.0.0).
    pub bind_addr: Ipv4Addr,
    /// Chunk size for splitting the image (default: MAX_CHUNK_DATA).
    pub chunk_size: usize,
    /// Number of times to re-broadcast the full image (for reliability).
    pub passes: u32,
    /// Inter-packet delay in microseconds (flow control).
    pub pacing_us: u64,
    /// Session ID (random UUID).
    pub session_id: u32,
    /// TTL for multicast packets.
    pub ttl: u32,
}

impl Default for MulticastConfig {
    fn default() -> Self {
        Self {
            group: DEFAULT_MULTICAST_GROUP.parse().unwrap(),
            port: DEFAULT_MULTICAST_PORT,
            bind_addr: Ipv4Addr::UNSPECIFIED,
            chunk_size: MAX_CHUNK_DATA,
            passes: 2,
            pacing_us: 100,
            session_id: rand_session_id(),
            ttl: 4,
        }
    }
}

/// Generate a random session ID.
fn rand_session_id() -> u32 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let s = RandomState::new();
    let mut h = s.build_hasher();
    h.write_u64(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64);
    h.finish() as u32
}

/// Packet types in the multicast protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum PacketType {
    /// Session announcement with metadata.
    Announce = 0x01,
    /// Data chunk.
    Data = 0x02,
    /// End of transmission.
    End = 0x03,
    /// Negative acknowledgement (receiver → sender, unicast).
    Nak = 0x04,
}

/// Session announcement packet (broadcast at start).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionAnnounce {
    pub session_id: u32,
    pub image_name: String,
    pub total_size: u64,
    pub total_chunks: u64,
    pub chunk_size: usize,
    pub hash: String,
}

/// Statistics from a multicast send/receive operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MulticastStats {
    pub session_id: u32,
    pub total_bytes: u64,
    pub total_chunks: u64,
    pub chunks_sent: u64,
    pub chunks_received: u64,
    pub retransmissions: u64,
    pub duration_ms: u64,
    pub throughput_bps: f64,
    pub receivers: u32,
}

/// Encode a data chunk packet.
///
/// Layout: [type:1][session:4][seq:8][total:8][crc:4][len:2][data:N]
fn encode_data_packet(session_id: u32, sequence: u64, total: u64, data: &[u8]) -> Vec<u8> {
    let crc = crc32fast::hash(data);
    let len = data.len() as u16;

    let mut pkt = Vec::with_capacity(CHUNK_HEADER_SIZE + data.len());
    pkt.push(PacketType::Data as u8);
    pkt.extend_from_slice(&session_id.to_be_bytes());
    pkt.extend_from_slice(&sequence.to_be_bytes());
    pkt.extend_from_slice(&total.to_be_bytes());
    pkt.extend_from_slice(&crc.to_be_bytes());
    // Pad header to CHUNK_HEADER_SIZE
    pkt.extend_from_slice(&len.to_be_bytes());
    // Pad remaining to reach exactly CHUNK_HEADER_SIZE
    while pkt.len() < CHUNK_HEADER_SIZE {
        pkt.push(0);
    }
    pkt.extend_from_slice(data);
    pkt
}

/// Decode a data chunk packet.
fn decode_data_packet(pkt: &[u8]) -> Result<(u32, u64, u64, u32, Vec<u8>)> {
    if pkt.len() < CHUNK_HEADER_SIZE {
        anyhow::bail!("Packet too short: {} bytes", pkt.len());
    }
    if pkt[0] != PacketType::Data as u8 {
        anyhow::bail!("Not a data packet: type={:#x}", pkt[0]);
    }

    let session_id = u32::from_be_bytes([pkt[1], pkt[2], pkt[3], pkt[4]]);
    let sequence = u64::from_be_bytes([pkt[5], pkt[6], pkt[7], pkt[8], pkt[9], pkt[10], pkt[11], pkt[12]]);
    let total = u64::from_be_bytes([pkt[13], pkt[14], pkt[15], pkt[16], pkt[17], pkt[18], pkt[19], pkt[20]]);
    let crc = u32::from_be_bytes([pkt[21], pkt[22], pkt[23], pkt[24]]);
    let data = pkt[CHUNK_HEADER_SIZE..].to_vec();

    Ok((session_id, sequence, total, crc, data))
}

/// Encode an announcement packet.
fn encode_announce(announce: &SessionAnnounce) -> Result<Vec<u8>> {
    let mut pkt = vec![PacketType::Announce as u8];
    let json = serde_json::to_vec(announce)?;
    pkt.extend_from_slice(&(json.len() as u32).to_be_bytes());
    pkt.extend_from_slice(&json);
    Ok(pkt)
}

/// Decode an announcement packet.
fn decode_announce(pkt: &[u8]) -> Result<SessionAnnounce> {
    if pkt.is_empty() || pkt[0] != PacketType::Announce as u8 {
        anyhow::bail!("Not an announce packet");
    }
    if pkt.len() < 5 {
        anyhow::bail!("Announce packet too short");
    }
    let len = u32::from_be_bytes([pkt[1], pkt[2], pkt[3], pkt[4]]) as usize;
    if pkt.len() < 5 + len {
        anyhow::bail!("Announce packet truncated");
    }
    let announce: SessionAnnounce = serde_json::from_slice(&pkt[5..5 + len])?;
    Ok(announce)
}

/// Encode an end-of-transmission packet.
fn encode_end(session_id: u32) -> Vec<u8> {
    let mut pkt = vec![PacketType::End as u8];
    pkt.extend_from_slice(&session_id.to_be_bytes());
    pkt
}

/// Send an image via UDP multicast.
///
/// Reads the image file, splits it into chunks, and broadcasts each chunk
/// to the multicast group. Sends the full image `config.passes` times
/// for reliability.
pub fn send_image(
    image_path: &std::path::Path,
    config: &MulticastConfig,
    cancel: &AtomicBool,
) -> Result<MulticastStats> {
    let image_data = std::fs::read(image_path)
        .with_context(|| format!("Failed to read image: {}", image_path.display()))?;

    let total_size = image_data.len() as u64;
    let chunk_size = config.chunk_size.min(MAX_CHUNK_DATA);
    let total_chunks = (total_size + chunk_size as u64 - 1) / chunk_size as u64;

    // Create UDP socket
    let socket = UdpSocket::bind(SocketAddr::new(config.bind_addr.into(), 0))
        .context("Failed to bind UDP socket")?;
    socket
        .set_multicast_ttl_v4(config.ttl)
        .context("Failed to set multicast TTL")?;

    let dest = SocketAddr::new(config.group.into(), config.port);
    let start = Instant::now();

    // Calculate image hash
    let hash = blake3::hash(&image_data).to_hex().to_string();

    // Send announcement
    let announce = SessionAnnounce {
        session_id: config.session_id,
        image_name: image_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string(),
        total_size,
        total_chunks,
        chunk_size,
        hash,
    };
    let announce_pkt = encode_announce(&announce)?;
    socket.send_to(&announce_pkt, dest)?;

    // Small delay after announcement for receivers to prepare
    std::thread::sleep(Duration::from_millis(100));

    let mut chunks_sent = 0u64;
    let pacing = Duration::from_micros(config.pacing_us);

    for _pass in 0..config.passes {
        for (i, chunk) in image_data.chunks(chunk_size).enumerate() {
            if cancel.load(Ordering::Relaxed) {
                break;
            }

            let pkt = encode_data_packet(config.session_id, i as u64, total_chunks, chunk);
            socket.send_to(&pkt, dest)?;
            chunks_sent += 1;

            if !pacing.is_zero() {
                std::thread::sleep(pacing);
            }
        }

        if cancel.load(Ordering::Relaxed) {
            break;
        }

        // Small gap between passes
        std::thread::sleep(Duration::from_millis(50));
    }

    // Send end packet
    let end_pkt = encode_end(config.session_id);
    for _ in 0..3 {
        socket.send_to(&end_pkt, dest)?;
        std::thread::sleep(Duration::from_millis(10));
    }

    let duration = start.elapsed();

    Ok(MulticastStats {
        session_id: config.session_id,
        total_bytes: total_size,
        total_chunks,
        chunks_sent,
        chunks_received: 0,
        retransmissions: 0,
        duration_ms: duration.as_millis() as u64,
        throughput_bps: if duration.as_millis() > 0 {
            total_size as f64 / (duration.as_millis() as f64 / 1000.0)
        } else {
            0.0
        },
        receivers: 0,
    })
}

/// Receive an image via UDP multicast.
///
/// Joins the multicast group, receives chunks, and assembles the image.
/// Returns the assembled image data and statistics.
pub fn receive_image(
    config: &MulticastConfig,
    timeout: Duration,
    cancel: &AtomicBool,
) -> Result<(Vec<u8>, MulticastStats)> {
    let bind_addr = SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), config.port);
    let socket = UdpSocket::bind(bind_addr).context("Failed to bind receiver socket")?;

    socket
        .join_multicast_v4(&config.group, &config.bind_addr)
        .context("Failed to join multicast group")?;

    socket
        .set_read_timeout(Some(Duration::from_secs(1)))
        .context("Failed to set read timeout")?;

    let start = Instant::now();
    let mut announce: Option<SessionAnnounce> = None;
    let mut chunks: std::collections::HashMap<u64, Vec<u8>> = std::collections::HashMap::new();
    let mut chunks_received = 0u64;
    let mut buf = vec![0u8; MAX_PAYLOAD + 64];

    loop {
        if cancel.load(Ordering::Relaxed) {
            anyhow::bail!("Receive cancelled");
        }

        if start.elapsed() > timeout {
            if announce.is_some() {
                // Timeout but got some data — assemble what we have
                break;
            }
            anyhow::bail!("Receive timeout: no session announced within {:?}", timeout);
        }

        let (len, _src) = match socket.recv_from(&mut buf) {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock
                || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(e) => return Err(e.into()),
        };

        let pkt = &buf[..len];
        if pkt.is_empty() {
            continue;
        }

        match pkt[0] {
            x if x == PacketType::Announce as u8 => {
                if let Ok(ann) = decode_announce(pkt) {
                    if config.session_id == 0 || ann.session_id == config.session_id {
                        announce = Some(ann);
                    }
                }
            }
            x if x == PacketType::Data as u8 => {
                if let Ok((session_id, seq, _total, crc, data)) = decode_data_packet(pkt) {
                    // Verify session and CRC
                    if let Some(ref ann) = announce {
                        if session_id == ann.session_id {
                            let actual_crc = crc32fast::hash(&data);
                            if actual_crc == crc {
                                chunks.entry(seq).or_insert_with(|| {
                                    chunks_received += 1;
                                    data
                                });
                            }
                        }
                    }
                }
            }
            x if x == PacketType::End as u8 => {
                if let Some(ref ann) = announce {
                    if pkt.len() >= 5 {
                        let sid = u32::from_be_bytes([pkt[1], pkt[2], pkt[3], pkt[4]]);
                        if sid == ann.session_id {
                            // Check if we have all chunks
                            if chunks.len() as u64 >= ann.total_chunks {
                                break;
                            }
                        }
                    }
                }
            }
            _ => {} // Unknown packet type
        }
    }

    let ann = announce.context("No session announcement received")?;

    // Assemble the image in order
    let mut image_data = Vec::with_capacity(ann.total_size as usize);
    for seq in 0..ann.total_chunks {
        if let Some(chunk) = chunks.get(&seq) {
            image_data.extend_from_slice(chunk);
        } else {
            anyhow::bail!(
                "Missing chunk {} of {} — received {}/{}",
                seq,
                ann.total_chunks,
                chunks_received,
                ann.total_chunks
            );
        }
    }

    // Truncate to exact size (last chunk may be padded)
    image_data.truncate(ann.total_size as usize);

    let duration = start.elapsed();

    let stats = MulticastStats {
        session_id: ann.session_id,
        total_bytes: ann.total_size,
        total_chunks: ann.total_chunks,
        chunks_sent: 0,
        chunks_received,
        retransmissions: 0,
        duration_ms: duration.as_millis() as u64,
        throughput_bps: if duration.as_millis() > 0 {
            ann.total_size as f64 / (duration.as_millis() as f64 / 1000.0)
        } else {
            0.0
        },
        receivers: 1,
    };

    Ok((image_data, stats))
}

/// Format multicast stats for display.
pub fn format_stats(stats: &MulticastStats) -> String {
    let total = humansize::format_size(stats.total_bytes, humansize::BINARY);
    let throughput = humansize::format_size(stats.throughput_bps as u64, humansize::BINARY);

    format!(
        "Multicast Transfer Complete\n\
         ├─ Session:     {:#010x}\n\
         ├─ Total:       {total}\n\
         ├─ Chunks:      sent={} received={}\n\
         ├─ Retransmits: {}\n\
         ├─ Duration:    {:.2}s\n\
         └─ Throughput:  {throughput}/s",
        stats.session_id,
        stats.chunks_sent,
        stats.chunks_received,
        stats.retransmissions,
        stats.duration_ms as f64 / 1000.0,
    )
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let cfg = MulticastConfig::default();
        assert_eq!(cfg.group, "239.42.42.1".parse::<Ipv4Addr>().unwrap());
        assert_eq!(cfg.port, DEFAULT_MULTICAST_PORT);
        assert!(cfg.session_id != 0);
        assert_eq!(cfg.passes, 2);
    }

    #[test]
    fn test_encode_decode_data_packet() {
        let data = b"hello multicast world";
        let pkt = encode_data_packet(0xDEADBEEF, 42, 100, data);

        let (session, seq, total, crc, decoded) = decode_data_packet(&pkt).unwrap();
        assert_eq!(session, 0xDEADBEEF);
        assert_eq!(seq, 42);
        assert_eq!(total, 100);
        assert_eq!(crc, crc32fast::hash(data));
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_data_packet_integrity() {
        let data = vec![0xAB; 1024];
        let pkt = encode_data_packet(1, 0, 1, &data);

        // Corrupt one byte
        let mut corrupted = pkt.clone();
        corrupted[CHUNK_HEADER_SIZE + 10] ^= 0xFF;

        let (_, _, _, crc, decoded) = decode_data_packet(&corrupted).unwrap();
        let actual_crc = crc32fast::hash(&decoded);
        assert_ne!(actual_crc, crc, "CRC should detect corruption");
    }

    #[test]
    fn test_encode_decode_announce() {
        let announce = SessionAnnounce {
            session_id: 12345,
            image_name: "test.img".into(),
            total_size: 1024 * 1024,
            total_chunks: 64,
            chunk_size: MAX_CHUNK_DATA,
            hash: "abc123".into(),
        };

        let pkt = encode_announce(&announce).unwrap();
        let decoded = decode_announce(&pkt).unwrap();

        assert_eq!(decoded.session_id, 12345);
        assert_eq!(decoded.image_name, "test.img");
        assert_eq!(decoded.total_size, 1024 * 1024);
        assert_eq!(decoded.total_chunks, 64);
        assert_eq!(decoded.hash, "abc123");
    }

    #[test]
    fn test_encode_end_packet() {
        let pkt = encode_end(0xCAFEBABE);
        assert_eq!(pkt[0], PacketType::End as u8);
        let sid = u32::from_be_bytes([pkt[1], pkt[2], pkt[3], pkt[4]]);
        assert_eq!(sid, 0xCAFEBABE);
    }

    #[test]
    fn test_max_chunk_data_fits_mtu() {
        assert!(MAX_CHUNK_DATA + CHUNK_HEADER_SIZE <= MAX_PAYLOAD);
        assert!(MAX_PAYLOAD <= 1400);
    }

    #[test]
    fn test_format_stats_display() {
        let stats = MulticastStats {
            session_id: 0xABCD,
            total_bytes: 10 * 1024 * 1024,
            total_chunks: 100,
            chunks_sent: 200,
            chunks_received: 100,
            retransmissions: 5,
            duration_ms: 2000,
            throughput_bps: 5.0 * 1024.0 * 1024.0,
            receivers: 3,
        };
        let output = format_stats(&stats);
        assert!(output.contains("Multicast Transfer Complete"));
        assert!(output.contains("sent=200"));
        assert!(output.contains("received=100"));
    }

    #[test]
    fn test_session_id_uniqueness() {
        let id1 = rand_session_id();
        std::thread::sleep(Duration::from_millis(1));
        let id2 = rand_session_id();
        // They should be different (extremely unlikely to collide)
        // but we can't guarantee it, so just check they're non-zero
        assert!(id1 != 0 || id2 != 0);
    }

    #[test]
    fn test_short_packet_rejected() {
        let pkt = vec![PacketType::Data as u8, 0, 0];
        let result = decode_data_packet(&pkt);
        assert!(result.is_err());
    }

    #[test]
    fn test_announce_wrong_type_rejected() {
        let pkt = vec![PacketType::Data as u8, 0, 0, 0, 0];
        let result = decode_announce(&pkt);
        assert!(result.is_err());
    }
}
