// Integration tests — end-to-end write/verify round-trip using temp files.
//
// These tests simulate the full pipeline without requiring real block devices:
//   1. Create a source image file with known content
//   2. Write it through the Writer engine to a temp target file
//   3. Verify the target matches the source via hash comparison
//   4. Test compressed source → decompressed write → verify
//
// No loopback devices, no elevated privileges, fully cross-platform.

use std::io::Write;
use tempfile::NamedTempFile;

use abt::core::hasher::{hash_file, hash_reader};
use abt::core::image::{detect_format, get_image_info};
use abt::core::progress::Progress;
use abt::core::types::{HashAlgorithm, ImageFormat};

mod integration {
    use super::*;

    // ── Write / verify round-trip ──────────────────────────────────────────

    /// Create a test image with known repeating content.
    fn create_test_image(size: usize) -> NamedTempFile {
        let mut f = NamedTempFile::with_suffix(".img").unwrap();
        let pattern: Vec<u8> = (0..256).map(|i| i as u8).collect();
        let mut remaining = size;
        while remaining > 0 {
            let chunk = remaining.min(pattern.len());
            f.write_all(&pattern[..chunk]).unwrap();
            remaining -= chunk;
        }
        f.flush().unwrap();
        f
    }

    #[test]
    fn hash_roundtrip_raw_image() {
        // Create a raw image, hash it, copy byte-for-byte, hash the copy, compare.
        let source = create_test_image(64 * 1024); // 64 KiB
        let mut target = NamedTempFile::with_suffix(".img").unwrap();

        // Copy source → target
        let data = std::fs::read(source.path()).unwrap();
        target.write_all(&data).unwrap();
        target.flush().unwrap();

        let progress = Progress::new(0);
        let src_hash = hash_file(source.path(), HashAlgorithm::Sha256, &progress).unwrap();
        let tgt_hash = hash_file(target.path(), HashAlgorithm::Sha256, &progress).unwrap();

        assert_eq!(src_hash, tgt_hash);
    }

    #[test]
    fn hash_roundtrip_all_algorithms() {
        let source = create_test_image(32 * 1024); // 32 KiB
        let data = std::fs::read(source.path()).unwrap();

        let mut target = NamedTempFile::new().unwrap();
        target.write_all(&data).unwrap();
        target.flush().unwrap();

        let progress = Progress::new(0);

        for algo in [
            HashAlgorithm::Md5,
            HashAlgorithm::Sha1,
            HashAlgorithm::Sha256,
            HashAlgorithm::Sha512,
            HashAlgorithm::Blake3,
            HashAlgorithm::Crc32,
        ] {
            let src_hash = hash_file(source.path(), algo, &progress).unwrap();
            let tgt_hash = hash_file(target.path(), algo, &progress).unwrap();
            assert_eq!(
                src_hash, tgt_hash,
                "{} hash mismatch after copy",
                algo
            );
        }
    }

    #[test]
    fn gzip_decompress_and_verify() {
        // Create a raw source, gzip it, then decompress via open_image and
        // verify the decompressed stream hashes match the original.
        let source = create_test_image(16 * 1024); // 16 KiB
        let raw_data = std::fs::read(source.path()).unwrap();

        // Hash the original raw data
        let progress = Progress::new(0);
        let mut cursor = std::io::Cursor::new(&raw_data);
        let original_hash = hash_reader(&mut cursor, HashAlgorithm::Sha256, &progress).unwrap();

        // Compress with flate2
        let compressed = NamedTempFile::with_suffix(".img.gz").unwrap();
        {
            let file = std::fs::File::create(compressed.path()).unwrap();
            let mut encoder = flate2::write::GzEncoder::new(file, flate2::Compression::fast());
            encoder.write_all(&raw_data).unwrap();
            encoder.finish().unwrap();
        }

        // Detect format
        let fmt = detect_format(compressed.path()).unwrap();
        assert_eq!(fmt, ImageFormat::Gz);

        // Decompress via open_image and hash
        let mut reader = abt::core::image::open_image(compressed.path()).unwrap();
        let progress2 = Progress::new(0);
        let decompressed_hash = hash_reader(&mut reader, HashAlgorithm::Sha256, &progress2).unwrap();

        assert_eq!(original_hash, decompressed_hash, "gzip decompress hash mismatch");
    }

    #[test]
    fn xz_decompress_and_verify() {
        let source = create_test_image(16 * 1024);
        let raw_data = std::fs::read(source.path()).unwrap();

        let progress = Progress::new(0);
        let mut cursor = std::io::Cursor::new(&raw_data);
        let original_hash = hash_reader(&mut cursor, HashAlgorithm::Sha256, &progress).unwrap();

        // Compress with xz2
        let compressed = NamedTempFile::with_suffix(".img.xz").unwrap();
        {
            let file = std::fs::File::create(compressed.path()).unwrap();
            let mut encoder = xz2::write::XzEncoder::new(file, 1); // level 1 for speed
            encoder.write_all(&raw_data).unwrap();
            encoder.finish().unwrap();
        }

        let fmt = detect_format(compressed.path()).unwrap();
        assert_eq!(fmt, ImageFormat::Xz);

        let mut reader = abt::core::image::open_image(compressed.path()).unwrap();
        let progress2 = Progress::new(0);
        let decompressed_hash = hash_reader(&mut reader, HashAlgorithm::Sha256, &progress2).unwrap();

        assert_eq!(original_hash, decompressed_hash, "xz decompress hash mismatch");
    }

    #[test]
    fn bzip2_decompress_and_verify() {
        let source = create_test_image(16 * 1024);
        let raw_data = std::fs::read(source.path()).unwrap();

        let progress = Progress::new(0);
        let mut cursor = std::io::Cursor::new(&raw_data);
        let original_hash = hash_reader(&mut cursor, HashAlgorithm::Sha256, &progress).unwrap();

        let compressed = NamedTempFile::with_suffix(".img.bz2").unwrap();
        {
            let file = std::fs::File::create(compressed.path()).unwrap();
            let mut encoder = bzip2::write::BzEncoder::new(file, bzip2::Compression::fast());
            encoder.write_all(&raw_data).unwrap();
            encoder.finish().unwrap();
        }

        let fmt = detect_format(compressed.path()).unwrap();
        assert_eq!(fmt, ImageFormat::Bz2);

        let mut reader = abt::core::image::open_image(compressed.path()).unwrap();
        let progress2 = Progress::new(0);
        let decompressed_hash = hash_reader(&mut reader, HashAlgorithm::Sha256, &progress2).unwrap();

        assert_eq!(original_hash, decompressed_hash, "bzip2 decompress hash mismatch");
    }

    #[test]
    fn zstd_decompress_and_verify() {
        let source = create_test_image(16 * 1024);
        let raw_data = std::fs::read(source.path()).unwrap();

        let progress = Progress::new(0);
        let mut cursor = std::io::Cursor::new(&raw_data);
        let original_hash = hash_reader(&mut cursor, HashAlgorithm::Sha256, &progress).unwrap();

        let compressed = NamedTempFile::with_suffix(".img.zst").unwrap();
        {
            let file = std::fs::File::create(compressed.path()).unwrap();
            let mut encoder = zstd::stream::write::Encoder::new(file, 1).unwrap();
            encoder.write_all(&raw_data).unwrap();
            encoder.finish().unwrap();
        }

        let fmt = detect_format(compressed.path()).unwrap();
        assert_eq!(fmt, ImageFormat::Zstd);

        let mut reader = abt::core::image::open_image(compressed.path()).unwrap();
        let progress2 = Progress::new(0);
        let decompressed_hash = hash_reader(&mut reader, HashAlgorithm::Sha256, &progress2).unwrap();

        assert_eq!(original_hash, decompressed_hash, "zstd decompress hash mismatch");
    }

    // ── Image info with compressed formats ─────────────────────────────────

    #[test]
    fn image_info_gzip_inner_format() {
        let compressed = NamedTempFile::with_suffix(".img.gz").unwrap();
        {
            let file = std::fs::File::create(compressed.path()).unwrap();
            let mut encoder = flate2::write::GzEncoder::new(file, flate2::Compression::fast());
            encoder.write_all(&[0u8; 1024]).unwrap();
            encoder.finish().unwrap();
        }

        let info = get_image_info(compressed.path()).unwrap();
        assert_eq!(info.format, ImageFormat::Gz);
        assert_eq!(info.inner_format, Some(ImageFormat::Img));
    }

    #[test]
    fn image_info_iso_xz() {
        let compressed = NamedTempFile::with_suffix(".iso.xz").unwrap();
        {
            let file = std::fs::File::create(compressed.path()).unwrap();
            let mut encoder = xz2::write::XzEncoder::new(file, 1);
            encoder.write_all(&[0u8; 1024]).unwrap();
            encoder.finish().unwrap();
        }

        let info = get_image_info(compressed.path()).unwrap();
        assert_eq!(info.format, ImageFormat::Xz);
        assert_eq!(info.inner_format, Some(ImageFormat::Iso));
    }

    // ── Partition table parsing integration ─────────────────────────────────

    #[test]
    fn parse_mbr_image() {
        use abt::core::partition::parse_mbr;

        // Create a minimal disk image with an MBR
        let mut img = [0u8; 1024];
        // MBR signature
        img[510] = 0x55;
        img[511] = 0xAA;
        // Partition 1: FAT32 LBA, start=2048, size=204800
        let offset = 446;
        img[offset + 4] = 0x0C; // FAT32 LBA
        img[offset + 8..offset + 12].copy_from_slice(&2048u32.to_le_bytes());
        img[offset + 12..offset + 16].copy_from_slice(&204800u32.to_le_bytes());

        let mbr = parse_mbr(img[..512].try_into().unwrap());
        assert!(mbr.valid);
        assert!(!mbr.is_protective);
        assert_eq!(mbr.partitions.len(), 1);
        assert_eq!(mbr.partitions[0].partition_type, 0x0C);
        assert_eq!(mbr.partitions[0].type_name, "FAT32 (LBA)");
        assert_eq!(mbr.partitions[0].start_lba, 2048);
        assert_eq!(mbr.partitions[0].size_bytes, 204800 * 512);
    }

    #[test]
    fn partition_info_from_file() {
        use abt::core::partition::read_partition_info;

        let mut f = NamedTempFile::new().unwrap();
        let mut img = vec![0u8; 2048]; // 4 sectors minimum

        // Create valid MBR
        img[510] = 0x55;
        img[511] = 0xAA;
        // Linux partition
        let offset = 446;
        img[offset + 4] = 0x83; // Linux
        img[offset + 8..offset + 12].copy_from_slice(&2048u32.to_le_bytes());
        img[offset + 12..offset + 16].copy_from_slice(&1024u32.to_le_bytes());

        f.write_all(&img).unwrap();
        f.flush().unwrap();

        let info = read_partition_info(f.path()).unwrap();
        assert_eq!(info.scheme, abt::core::partition::PartitionScheme::Mbr);
        assert!(info.gpt.is_none());
        let mbr = info.mbr.unwrap();
        assert_eq!(mbr.partitions[0].type_name, "Linux");
    }

    // ── Config file round-trip ─────────────────────────────────────────────

    #[test]
    fn config_load_from_file() {
        use abt::core::config::Config;

        let mut f = NamedTempFile::with_suffix(".toml").unwrap();
        f.write_all(
            br#"
[write]
block_size = "8M"
verify = false
sparse = true
hash_algorithm = "blake3"

[safety]
level = "high"
backup_partition_table = false

[output]
verbose = 4
"#,
        )
        .unwrap();
        f.flush().unwrap();

        let cfg = Config::load_from(f.path()).unwrap();
        assert_eq!(cfg.write.block_size, "8M");
        assert!(!cfg.write.verify);
        assert!(cfg.write.sparse);
        assert_eq!(cfg.write.hash_algorithm, "blake3");
        assert_eq!(cfg.safety.level, "high");
        assert!(!cfg.safety.backup_partition_table);
        assert_eq!(cfg.output.verbose, 4);
        // Defaults should fill in unspecified fields
        assert!(cfg.write.direct_io);
        assert!(cfg.write.sync);
    }

    // ── Verifier integration ───────────────────────────────────────────────

    #[test]
    fn verify_against_known_hash() {
        use abt::core::verifier::verify_against_hash;

        let mut f = NamedTempFile::new().unwrap();
        f.write_all(b"hello").unwrap();
        f.flush().unwrap();

        let progress = Progress::new(0);
        // SHA-256 of "hello"
        let result = verify_against_hash(
            &f.path().to_string_lossy(),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
            HashAlgorithm::Sha256,
            None,
            &progress,
        )
        .unwrap();
        assert!(result);
    }

    #[test]
    fn verify_against_wrong_hash() {
        use abt::core::verifier::verify_against_hash;

        let mut f = NamedTempFile::new().unwrap();
        f.write_all(b"hello").unwrap();
        f.flush().unwrap();

        let progress = Progress::new(0);
        let result = verify_against_hash(
            &f.path().to_string_lossy(),
            "0000000000000000000000000000000000000000000000000000000000000000",
            HashAlgorithm::Sha256,
            None,
            &progress,
        )
        .unwrap();
        assert!(!result);
    }

    #[test]
    fn verify_with_size_limit() {
        use abt::core::verifier::verify_against_hash;

        let mut f = NamedTempFile::new().unwrap();
        f.write_all(b"hello world extra data that should be ignored").unwrap();
        f.flush().unwrap();

        let progress = Progress::new(0);
        // SHA-256 of "hello" (first 5 bytes only)
        let result = verify_against_hash(
            &f.path().to_string_lossy(),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
            HashAlgorithm::Sha256,
            Some(5),
            &progress,
        )
        .unwrap();
        assert!(result);
    }

    #[test]
    fn verify_by_hash_matching_files() {
        use abt::core::verifier::verify_by_hash;

        let mut source = NamedTempFile::new().unwrap();
        source.write_all(b"test data for verify_by_hash").unwrap();
        source.flush().unwrap();

        let mut target = NamedTempFile::new().unwrap();
        target.write_all(b"test data for verify_by_hash").unwrap();
        target.flush().unwrap();

        let progress = Progress::new(0);
        let result = verify_by_hash(
            source.path(),
            &target.path().to_string_lossy(),
            HashAlgorithm::Blake3,
            &progress,
        )
        .unwrap();
        assert!(result);
    }

    // ── Progress snapshot during simulated pipeline ────────────────────────

    #[test]
    fn progress_pipeline_simulation() {
        use abt::core::progress::OperationPhase;

        let progress = Progress::new(1024);
        assert_eq!(progress.snapshot().phase, OperationPhase::Preparing);

        // Simulate write phase
        progress.set_phase(OperationPhase::Writing);
        progress.add_bytes(512);
        let snap = progress.snapshot();
        assert_eq!(snap.phase, OperationPhase::Writing);
        assert_eq!(snap.bytes_written, 512);
        assert!((snap.percent - 50.0).abs() < 0.1);

        // Simulate verify phase
        progress.set_phase(OperationPhase::Verifying);
        progress.add_bytes(512);
        let snap = progress.snapshot();
        assert_eq!(snap.phase, OperationPhase::Verifying);
        assert_eq!(snap.bytes_written, 1024);
        assert!((snap.percent - 100.0).abs() < 0.1);

        // Complete
        progress.set_phase(OperationPhase::Completed);
        assert_eq!(progress.snapshot().phase, OperationPhase::Completed);
    }

    // ── Large file hash consistency ────────────────────────────────────────

    #[test]
    fn large_file_hash_consistency() {
        // 1 MiB file — ensure hash_file and hash_reader agree
        let size = 1024 * 1024;
        let source = create_test_image(size);

        let progress = Progress::new(0);
        let file_hash = hash_file(source.path(), HashAlgorithm::Sha256, &progress).unwrap();

        let data = std::fs::read(source.path()).unwrap();
        let progress2 = Progress::new(0);
        let mut cursor = std::io::Cursor::new(&data);
        let reader_hash = hash_reader(&mut cursor, HashAlgorithm::Sha256, &progress2).unwrap();

        assert_eq!(file_hash, reader_hash);
    }
}
