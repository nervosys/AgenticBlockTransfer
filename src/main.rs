// AgenticBlockTransfer (abt) — Cross-platform agentic block transfer
// Copyright (c) nervosys. Licensed under MIT OR Apache-2.0.

use anyhow::Result;
use clap::Parser;
use log::{error, info};
use std::sync::Arc;

mod cli;
mod core;
mod mcp;
mod ontology;
mod platform;

#[cfg(feature = "tui")]
mod tui;

#[cfg(feature = "gui")]
mod gui;

use cli::Args;
use core::progress::Progress;

/// Sets up a JSON-structured log file. Returns a guard that flushes on drop.
/// Each line is a JSON object: {"ts":"...","level":"INFO","target":"abt::core::writer","msg":"..."}
fn setup_file_logger(path: &str) -> Result<std::fs::File> {
    use std::io::Write;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;

    // Write a session header
    let mut f = file.try_clone()?;
    writeln!(
        f,
        "{{\"ts\":\"{}\",\"level\":\"INFO\",\"target\":\"abt\",\"msg\":\"Log session started — abt v{}\"}}",
        chrono::Utc::now().to_rfc3339(),
        env!("CARGO_PKG_VERSION")
    )?;

    info!("Logging to file: {}", path);
    Ok(file)
}

/// Global signal handler — sets the cancel flag on Ctrl+C so that all in-flight
/// operations (write, download, verify) observe it and shut down gracefully.
/// A second Ctrl+C force-exits (process::exit) for emergencies.
fn install_signal_handler(progress: Arc<Progress>) {
    tokio::spawn(async move {
        // First Ctrl+C: graceful cancel
        if tokio::signal::ctrl_c().await.is_ok() {
            error!("Interrupt received — cancelling operation...");
            progress.cancel();

            // Second Ctrl+C: force exit
            if tokio::signal::ctrl_c().await.is_ok() {
                error!("Second interrupt — forcing exit");
                std::process::exit(130); // 128 + SIGINT
            }
        }
    });
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    env_logger::Builder::new()
        .filter_level(args.log_level())
        .format_timestamp_millis()
        .init();

    // If --log-file is specified, spawn a structured JSON log writer.
    // The env_logger writes to stderr; we additionally write JSON lines to the file.
    let _log_guard = if let Some(ref log_path) = args.log_file {
        Some(setup_file_logger(log_path)?)
    } else {
        None
    };

    info!("abt v{}", env!("CARGO_PKG_VERSION"));

    // Install global Ctrl+C handler for graceful shutdown.
    // The Progress handle is shared: commands that perform long-running work
    // check `progress.is_cancelled()` on every block / chunk.
    let signal_progress = Arc::new(Progress::new(0));
    install_signal_handler(signal_progress.clone());

    match args.command {
        cli::Command::Write(opts) => cli::commands::write::execute(opts, &args.output).await,
        cli::Command::Verify(opts) => cli::commands::verify::execute(opts).await,
        cli::Command::List(opts) => cli::commands::list::execute(opts, &args.output).await,
        cli::Command::Info(opts) => cli::commands::info::execute(opts).await,
        cli::Command::Checksum(opts) => cli::commands::checksum::execute(opts).await,
        cli::Command::Format(opts) => cli::commands::format::execute(opts).await,
        cli::Command::Ontology(opts) => cli::commands::ontology::execute(opts),
        cli::Command::Completions(opts) => cli::commands::completions::execute(opts),
        cli::Command::Man(opts) => cli::commands::man::execute(opts),
        #[cfg(feature = "tui")]
        cli::Command::Tui => tui::run().await,
        #[cfg(feature = "gui")]
        cli::Command::Gui => gui::run(),
        cli::Command::Mcp(opts) => {
            mcp::run_server(opts.oneshot)?;
            Ok(())
        }
        cli::Command::Clone(opts) => cli::commands::clone::execute(opts).await,
        cli::Command::Erase(opts) => cli::commands::erase::execute(opts).await,
        cli::Command::Boot(opts) => cli::commands::boot::execute(opts),
        cli::Command::Catalog(opts) => cli::commands::catalog::execute(opts).await,
        cli::Command::Bench(opts) => cli::commands::bench::execute(opts).await,
        cli::Command::Diff(opts) => cli::commands::diff::execute(opts).await,
        cli::Command::Multiboot(opts) => cli::commands::multiboot::execute(opts).await,
        cli::Command::Customize(opts) => cli::commands::customize::execute(opts).await,
        cli::Command::Cache(opts) => cli::commands::cache::execute(opts).await,
        cli::Command::Health(opts) => cli::commands::health::execute(opts).await,
        cli::Command::Backup(opts) => cli::commands::backup::execute(opts).await,
        cli::Command::Persist(opts) => cli::commands::persist::execute(opts).await,
        cli::Command::Update(opts) => cli::commands::update::execute(opts).await,
        cli::Command::Mirror(opts) => cli::commands::mirror::execute(opts).await,
        cli::Command::ChecksumFile(opts) => cli::commands::checksum_file::execute(opts).await,
        cli::Command::UsbInfo(opts) => cli::commands::usb_info::execute(opts).await,
        cli::Command::Signature(opts) => cli::commands::signature::execute(opts).await,
        cli::Command::Wue(opts) => cli::commands::wue::execute(opts).await,
        cli::Command::UefiNtfs(opts) => cli::commands::uefi_ntfs::execute(opts).await,
        cli::Command::Fleet(opts) => cli::commands::fleet::execute(opts).await,
        cli::Command::Restore(opts) => cli::commands::restore::execute(opts).await,
        cli::Command::Telemetry(opts) => cli::commands::telemetry::execute(opts).await,
        cli::Command::Watchdog(opts) => cli::commands::watchdog::execute(opts).await,
        cli::Command::WimExtract(opts) => cli::commands::wim_extract::execute(opts).await,
        cli::Command::SecureBoot(opts) => cli::commands::secureboot::execute(opts).await,
        cli::Command::FsDetect(opts) => cli::commands::fs_detect::execute(opts).await,
        cli::Command::DriveScan(opts) => cli::commands::drive_scan::execute(opts).await,
        cli::Command::DriveConstraints(opts) => cli::commands::drive_constraints::execute(opts).await,
        cli::Command::WinToGo(opts) => cli::commands::wintogo::execute(opts).await,
        cli::Command::Syslinux(opts) => cli::commands::syslinux::execute(opts).await,
        cli::Command::Ffu(opts) => cli::commands::ffu::execute(opts).await,
        cli::Command::IsoHybrid(opts) => cli::commands::isohybrid::execute(opts).await,
        cli::Command::ProcLock(opts) => cli::commands::proclock::execute(opts).await,
        cli::Command::Elevate(opts) => cli::commands::elevate::execute(opts).await,
        cli::Command::Optical(opts) => cli::commands::optical::execute(opts).await,
    }
}
