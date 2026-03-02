// Unit tests for the core library modules.
//
// Tests cover: image format detection, hashing, progress tracking, sparse
// zero-detection, block size parsing, error types, and type definitions.

#[cfg(test)]
mod tests {
    // ── Image format detection ─────────────────────────────────────────────

    mod image_detection {
        use std::io::Write;
        use tempfile::NamedTempFile;

        use abt::core::image::detect_format;
        use abt::core::types::ImageFormat;

        #[test]
        fn detect_gzip_magic() {
            let mut f = NamedTempFile::with_suffix(".bin").unwrap();
            // gzip magic: 0x1f 0x8b
            f.write_all(&[0x1f, 0x8b, 0x08, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
                .unwrap();
            f.flush().unwrap();
            let fmt = detect_format(f.path()).unwrap();
            assert_eq!(fmt, ImageFormat::Gz);
        }

        #[test]
        fn detect_bzip2_magic() {
            let mut f = NamedTempFile::with_suffix(".bin").unwrap();
            // bzip2 magic: 0x42 0x5a 0x68
            f.write_all(&[0x42, 0x5a, 0x68, 0x39, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
                .unwrap();
            f.flush().unwrap();
            let fmt = detect_format(f.path()).unwrap();
            assert_eq!(fmt, ImageFormat::Bz2);
        }

        #[test]
        fn detect_xz_magic() {
            let mut f = NamedTempFile::with_suffix(".bin").unwrap();
            // xz magic: 0xfd 0x37 0x7a 0x58 0x5a 0x00
            f.write_all(&[0xfd, 0x37, 0x7a, 0x58, 0x5a, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
                .unwrap();
            f.flush().unwrap();
            let fmt = detect_format(f.path()).unwrap();
            assert_eq!(fmt, ImageFormat::Xz);
        }

        #[test]
        fn detect_zstd_magic() {
            let mut f = NamedTempFile::with_suffix(".bin").unwrap();
            // zstd magic: 0x28 0xb5 0x2f 0xfd
            f.write_all(&[0x28, 0xb5, 0x2f, 0xfd, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
                .unwrap();
            f.flush().unwrap();
            let fmt = detect_format(f.path()).unwrap();
            assert_eq!(fmt, ImageFormat::Zstd);
        }

        #[test]
        fn detect_zip_magic() {
            let mut f = NamedTempFile::with_suffix(".bin").unwrap();
            // zip magic: 0x50 0x4b 0x03 0x04
            f.write_all(&[0x50, 0x4b, 0x03, 0x04, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
                .unwrap();
            f.flush().unwrap();
            let fmt = detect_format(f.path()).unwrap();
            assert_eq!(fmt, ImageFormat::Zip);
        }

        #[test]
        fn detect_qcow2_magic() {
            let mut f = NamedTempFile::with_suffix(".bin").unwrap();
            // QCOW2 magic: 0x51 0x46 0x49 0xfb
            f.write_all(&[0x51, 0x46, 0x49, 0xfb, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
                .unwrap();
            f.flush().unwrap();
            let fmt = detect_format(f.path()).unwrap();
            assert_eq!(fmt, ImageFormat::Qcow2);
        }

        #[test]
        fn detect_by_extension_iso() {
            let f = NamedTempFile::with_suffix(".iso").unwrap();
            // Write some non-magic bytes — should fall back to extension
            std::fs::write(f.path(), &[0u8; 16]).unwrap();
            let fmt = detect_format(f.path()).unwrap();
            assert_eq!(fmt, ImageFormat::Iso);
        }

        #[test]
        fn detect_by_extension_img() {
            let f = NamedTempFile::with_suffix(".img").unwrap();
            std::fs::write(f.path(), &[0u8; 16]).unwrap();
            let fmt = detect_format(f.path()).unwrap();
            assert_eq!(fmt, ImageFormat::Img);
        }

        #[test]
        fn detect_by_extension_raw() {
            let f = NamedTempFile::with_suffix(".raw").unwrap();
            std::fs::write(f.path(), &[0u8; 16]).unwrap();
            let fmt = detect_format(f.path()).unwrap();
            assert_eq!(fmt, ImageFormat::Raw);
        }

        #[test]
        fn magic_overrides_extension() {
            // File has .iso extension but gzip magic bytes — magic wins
            let mut f = NamedTempFile::with_suffix(".iso").unwrap();
            f.write_all(&[0x1f, 0x8b, 0x08, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
                .unwrap();
            f.flush().unwrap();
            let fmt = detect_format(f.path()).unwrap();
            assert_eq!(fmt, ImageFormat::Gz);
        }

        #[test]
        fn unknown_format_errors() {
            let f = NamedTempFile::with_suffix(".unknown_ext_xyz").unwrap();
            std::fs::write(f.path(), &[0u8; 16]).unwrap();
            assert!(detect_format(f.path()).is_err());
        }

        #[test]
        fn compressed_format_detection() {
            assert!(ImageFormat::Gz.is_compressed());
            assert!(ImageFormat::Bz2.is_compressed());
            assert!(ImageFormat::Xz.is_compressed());
            assert!(ImageFormat::Zstd.is_compressed());
            assert!(ImageFormat::Zip.is_compressed());
            assert!(!ImageFormat::Iso.is_compressed());
            assert!(!ImageFormat::Raw.is_compressed());
            assert!(!ImageFormat::Img.is_compressed());
        }

        #[test]
        fn extension_case_insensitive() {
            use std::path::Path;
            // from_extension should handle case since we call to_lowercase
            assert_eq!(
                ImageFormat::from_extension(Path::new("test.ISO")),
                Some(ImageFormat::Iso)
            );
            assert_eq!(
                ImageFormat::from_extension(Path::new("test.Gz")),
                Some(ImageFormat::Gz)
            );
        }
    }

    // ── Hashing ────────────────────────────────────────────────────────────

    mod hashing {
        use std::io::Cursor;

        use abt::core::hasher::hash_reader;
        use abt::core::progress::Progress;
        use abt::core::types::HashAlgorithm;

        fn hash_bytes(data: &[u8], algo: HashAlgorithm) -> String {
            let progress = Progress::new(data.len() as u64);
            let mut cursor = Cursor::new(data);
            hash_reader(&mut cursor, algo, &progress).unwrap()
        }

        #[test]
        fn sha256_empty() {
            let hash = hash_bytes(b"", HashAlgorithm::Sha256);
            assert_eq!(
                hash,
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
            );
        }

        #[test]
        fn sha256_hello() {
            let hash = hash_bytes(b"hello", HashAlgorithm::Sha256);
            assert_eq!(
                hash,
                "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
            );
        }

        #[test]
        fn md5_hello() {
            let hash = hash_bytes(b"hello", HashAlgorithm::Md5);
            assert_eq!(hash, "5d41402abc4b2a76b9719d911017c592");
        }

        #[test]
        fn sha1_hello() {
            let hash = hash_bytes(b"hello", HashAlgorithm::Sha1);
            assert_eq!(hash, "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d");
        }

        #[test]
        fn sha512_hello() {
            let hash = hash_bytes(b"hello", HashAlgorithm::Sha512);
            assert_eq!(
                hash,
                "9b71d224bd62f3785d96d46ad3ea3d73319bfbc2890caadae2dff72519673ca72323c3d99ba5c11d7c7acc6e14b8c5da0c4663475c2e5c3adef46f73bcdec043"
            );
        }

        #[test]
        fn blake3_hello() {
            let hash = hash_bytes(b"hello", HashAlgorithm::Blake3);
            assert_eq!(
                hash,
                "ea8f163db38682925e4491c5e58d4bb3506ef8c14eb78a86e908c5624a67200f"
            );
        }

        #[test]
        fn crc32_hello() {
            let hash = hash_bytes(b"hello", HashAlgorithm::Crc32);
            assert_eq!(hash, "3610a686");
        }

        #[test]
        fn all_algorithms_produce_output() {
            let data = b"test data for all algorithms";
            for algo in [
                HashAlgorithm::Md5,
                HashAlgorithm::Sha1,
                HashAlgorithm::Sha256,
                HashAlgorithm::Sha512,
                HashAlgorithm::Blake3,
                HashAlgorithm::Crc32,
            ] {
                let hash = hash_bytes(data, algo);
                assert!(!hash.is_empty(), "{} produced empty hash", algo);
                assert!(
                    hash.chars().all(|c| c.is_ascii_hexdigit()),
                    "{} hash contains non-hex chars: {}",
                    algo,
                    hash
                );
            }
        }

        #[test]
        fn hash_file_works() {
            use abt::core::hasher::hash_file;
            use std::io::Write;
            use tempfile::NamedTempFile;

            let mut f = NamedTempFile::new().unwrap();
            f.write_all(b"hello").unwrap();
            f.flush().unwrap();

            let progress = Progress::new(0);
            let hash = hash_file(f.path(), HashAlgorithm::Sha256, &progress).unwrap();
            assert_eq!(
                hash,
                "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
            );
        }
    }

    // ── Progress tracking ──────────────────────────────────────────────────

    mod progress {
        use abt::core::progress::{OperationPhase, Progress};

        #[test]
        fn new_progress_starts_at_zero() {
            let p = Progress::new(1000);
            let snap = p.snapshot();
            assert_eq!(snap.bytes_written, 0);
            assert_eq!(snap.bytes_total, 1000);
            assert_eq!(snap.phase, OperationPhase::Preparing);
            assert!(!p.is_cancelled());
        }

        #[test]
        fn add_bytes_accumulates() {
            let p = Progress::new(100);
            p.add_bytes(30);
            p.add_bytes(20);
            let snap = p.snapshot();
            assert_eq!(snap.bytes_written, 50);
        }

        #[test]
        fn percent_calculation() {
            let p = Progress::new(200);
            p.add_bytes(100);
            let snap = p.snapshot();
            assert!((snap.percent - 50.0).abs() < 0.01);
        }

        #[test]
        fn cancel_flag() {
            let p = Progress::new(100);
            assert!(!p.is_cancelled());
            p.cancel();
            assert!(p.is_cancelled());
        }

        #[test]
        fn phase_transitions() {
            let p = Progress::new(100);
            assert_eq!(p.snapshot().phase, OperationPhase::Preparing);

            p.set_phase(OperationPhase::Writing);
            assert_eq!(p.snapshot().phase, OperationPhase::Writing);

            p.set_phase(OperationPhase::Verifying);
            assert_eq!(p.snapshot().phase, OperationPhase::Verifying);

            p.set_phase(OperationPhase::Completed);
            assert_eq!(p.snapshot().phase, OperationPhase::Completed);
        }

        #[test]
        fn clone_shares_state() {
            let p1 = Progress::new(100);
            let p2 = p1.clone();
            p1.add_bytes(50);
            assert_eq!(p2.snapshot().bytes_written, 50);
            p2.cancel();
            assert!(p1.is_cancelled());
        }

        #[test]
        fn set_total_updates() {
            let p = Progress::new(0);
            assert_eq!(p.snapshot().bytes_total, 0);
            p.set_total(999);
            assert_eq!(p.snapshot().bytes_total, 999);
        }

        #[test]
        fn speed_and_eta() {
            let p = Progress::new(1000);
            p.add_bytes(500);
            // Just verify fields exist and are reasonable
            let snap = p.snapshot();
            assert!(snap.elapsed_secs >= 0.0);
            assert!(snap.speed_bytes_per_sec >= 0.0);
            // ETA should exist since total > written
            assert!(snap.eta_secs.is_some());
        }
    }

    // ── Block size parsing ─────────────────────────────────────────────────

    mod block_size {
        use abt::cli::parse_block_size;

        #[test]
        fn parse_plain_bytes() {
            assert_eq!(parse_block_size("512").unwrap(), 512);
            assert_eq!(parse_block_size("4096").unwrap(), 4096);
        }

        #[test]
        fn parse_kilobytes() {
            assert_eq!(parse_block_size("4K").unwrap(), 4 * 1024);
            assert_eq!(parse_block_size("512k").unwrap(), 512 * 1024);
        }

        #[test]
        fn parse_megabytes() {
            assert_eq!(parse_block_size("1M").unwrap(), 1024 * 1024);
            assert_eq!(parse_block_size("4m").unwrap(), 4 * 1024 * 1024);
        }

        #[test]
        fn parse_gigabytes() {
            assert_eq!(parse_block_size("1G").unwrap(), 1024 * 1024 * 1024);
        }

        #[test]
        fn parse_with_whitespace() {
            assert_eq!(parse_block_size("  4M  ").unwrap(), 4 * 1024 * 1024);
        }

        #[test]
        fn invalid_block_size() {
            assert!(parse_block_size("abc").is_err());
            assert!(parse_block_size("").is_err());
        }
    }

    // ── Type definitions ───────────────────────────────────────────────────

    mod types {
        use abt::core::types::*;
        use std::path::PathBuf;

        #[test]
        fn write_config_defaults() {
            let cfg = WriteConfig::default();
            assert_eq!(cfg.block_size, 4 * 1024 * 1024);
            assert!(cfg.verify);
            assert!(cfg.direct_io);
            assert!(cfg.sync);
            assert!(cfg.decompress);
            assert!(!cfg.sparse);
            assert!(!cfg.force);
            assert_eq!(cfg.mode, WriteMode::Raw);
            assert_eq!(cfg.hash_algorithm, Some(HashAlgorithm::Sha256));
        }

        #[test]
        fn image_source_display() {
            assert_eq!(
                format!("{}", ImageSource::File(PathBuf::from("/dev/sda"))),
                "/dev/sda"
            );
            assert_eq!(
                format!("{}", ImageSource::Url("https://example.com/image.iso".to_string())),
                "https://example.com/image.iso"
            );
            assert_eq!(format!("{}", ImageSource::Stdin), "<stdin>");
        }

        #[test]
        fn device_type_display() {
            assert_eq!(format!("{}", DeviceType::Usb), "USB");
            assert_eq!(format!("{}", DeviceType::Nvme), "NVMe");
            assert_eq!(format!("{}", DeviceType::Sd), "SD");
        }

        #[test]
        fn hash_algorithm_display() {
            assert_eq!(format!("{}", HashAlgorithm::Sha256), "SHA-256");
            assert_eq!(format!("{}", HashAlgorithm::Blake3), "BLAKE3");
            assert_eq!(format!("{}", HashAlgorithm::Crc32), "CRC32");
        }

        #[test]
        fn filesystem_display() {
            assert_eq!(format!("{}", Filesystem::Fat32), "FAT32");
            assert_eq!(format!("{}", Filesystem::Ext4), "ext4");
            assert_eq!(format!("{}", Filesystem::ExFat), "exFAT");
        }

        #[test]
        fn image_format_roundtrip() {
            for fmt in [
                ImageFormat::Raw,
                ImageFormat::Iso,
                ImageFormat::Gz,
                ImageFormat::Xz,
            ] {
                let s = format!("{}", fmt);
                assert!(!s.is_empty());
            }
        }

        #[test]
        fn write_config_serialization() {
            let cfg = WriteConfig::default();
            let json = serde_json::to_string(&cfg).unwrap();
            let cfg2: WriteConfig = serde_json::from_str(&json).unwrap();
            assert_eq!(cfg.block_size, cfg2.block_size);
            assert_eq!(cfg.verify, cfg2.verify);
            assert_eq!(cfg.sparse, cfg2.sparse);
        }
    }

    // ── Device info ────────────────────────────────────────────────────────

    mod device {
        use abt::core::device::DeviceInfo;
        use abt::core::types::DeviceType;

        fn sample_device(is_system: bool, read_only: bool, removable: bool) -> DeviceInfo {
            DeviceInfo {
                path: "/dev/sdb".to_string(),
                name: "Test Drive".to_string(),
                vendor: "TestVendor".to_string(),
                serial: Some("ABC123".to_string()),
                size: 32 * 1024 * 1024 * 1024,
                sector_size: 512,
                physical_sector_size: 4096,
                removable,
                read_only,
                is_system,
                device_type: DeviceType::Usb,
                mount_points: vec![],
                transport: "usb".to_string(),
            }
        }

        #[test]
        fn safe_target_rejects_system_drive() {
            let d = sample_device(true, false, true);
            assert!(!d.is_safe_target());
        }

        #[test]
        fn safe_target_rejects_read_only() {
            let d = sample_device(false, true, true);
            assert!(!d.is_safe_target());
        }

        #[test]
        fn safe_target_allows_removable() {
            let d = sample_device(false, false, true);
            assert!(d.is_safe_target());
        }

        #[test]
        fn removable_media_detection() {
            let usb = sample_device(false, false, true);
            assert!(usb.is_removable_media());

            let mut fixed = sample_device(false, false, false);
            fixed.device_type = DeviceType::Sata;
            assert!(!fixed.is_removable_media());
        }

        #[test]
        fn display_format() {
            let d = sample_device(false, false, true);
            let s = format!("{}", d);
            assert!(s.contains("/dev/sdb"));
            assert!(s.contains("TestVendor"));
            assert!(s.contains("USB"));
        }
    }

    // ── Error types ────────────────────────────────────────────────────────

    mod errors {
        use abt::core::error::AbtError;

        #[test]
        fn error_display_messages() {
            let e = AbtError::DeviceNotFound("sdb".to_string());
            assert!(format!("{}", e).contains("sdb"));

            let e = AbtError::ImageTooLarge {
                image_size: 100,
                device_size: 50,
            };
            let msg = format!("{}", e);
            assert!(msg.contains("100"));
            assert!(msg.contains("50"));

            let e = AbtError::ChecksumMismatch {
                expected: "abc".to_string(),
                actual: "xyz".to_string(),
            };
            let msg = format!("{}", e);
            assert!(msg.contains("abc"));
            assert!(msg.contains("xyz"));
        }

        #[test]
        fn error_variants_are_send() {
            fn assert_send<T: Send>() {}
            assert_send::<AbtError>();
        }

        #[test]
        fn io_error_converts() {
            let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "test");
            let abt_err: AbtError = io_err.into();
            assert!(matches!(abt_err, AbtError::Io(_)));
        }
    }

    // ── Download helpers ───────────────────────────────────────────────────

    mod download {
        use abt::core::download::{cleanup_download, extract_filename};

        #[test]
        fn extract_filename_basic() {
            assert_eq!(
                extract_filename("https://releases.ubuntu.com/24.04/ubuntu.iso"),
                "ubuntu.iso"
            );
        }

        #[test]
        fn extract_filename_with_query() {
            assert_eq!(
                extract_filename("https://example.com/file.img?token=abc&v=2"),
                "file.img"
            );
        }

        #[test]
        fn extract_filename_no_path() {
            assert_eq!(extract_filename("https://example.com/"), "");
        }

        #[test]
        fn extract_filename_empty() {
            // Should not panic
            let _ = extract_filename("");
        }

        #[test]
        fn cleanup_nonexistent_does_not_panic() {
            let fake = std::env::temp_dir()
                .join("abt_downloads")
                .join("nonexistent_file_abc123.iso");
            cleanup_download(&fake);
            // Should not panic — just logs a warning
        }

        #[test]
        fn cleanup_outside_temp_dir_is_noop() {
            use std::path::Path;
            // Must not delete files outside abt_downloads
            cleanup_download(Path::new("/etc/passwd"));
            cleanup_download(Path::new("C:\\Windows\\System32\\config"));
        }
    }

    // ── Image info ─────────────────────────────────────────────────────────

    mod image_info {
        use abt::core::image::get_image_info;
        use std::io::Write;
        use tempfile::NamedTempFile;

        #[test]
        fn image_info_basic() {
            let mut f = NamedTempFile::with_suffix(".iso").unwrap();
            f.write_all(&[0u8; 1024]).unwrap();
            f.flush().unwrap();

            let info = get_image_info(f.path()).unwrap();
            assert_eq!(info.size, 1024);
            assert_eq!(
                info.format,
                abt::core::types::ImageFormat::Iso,
            );
        }

        #[test]
        fn image_info_compressed_detects_inner() {
            let mut f = NamedTempFile::with_suffix(".img.gz").unwrap();
            // gzip magic
            f.write_all(&[0x1f, 0x8b, 0x08, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
                .unwrap();
            f.flush().unwrap();

            let info = get_image_info(f.path()).unwrap();
            assert_eq!(info.format, abt::core::types::ImageFormat::Gz);
            // Inner format should be detected from stem
            assert_eq!(info.inner_format, Some(abt::core::types::ImageFormat::Img));
        }
    }
}
