use anyhow::Result;

use crate::cli::ProcLockOpts;
use crate::core::proclock;

pub async fn execute(opts: ProcLockOpts) -> Result<()> {
    match opts.action.as_str() {
        "scan" | "check" | "detect" => {
            let device = &opts.device;

            let config = proclock::LockScanConfig {
                include_system: opts.include_system,
                resolve_mounts: !opts.no_resolve_mounts,
                timeout_ms: opts.timeout.unwrap_or(5000),
                ..Default::default()
            };

            let result = proclock::scan_locks(device, &config)?;

            if opts.json {
                let json = serde_json::json!({
                    "target": result.target,
                    "has_locks": result.has_locks(),
                    "process_count": result.process_count(),
                    "total_handles": result.total_handles,
                    "has_critical": result.has_critical,
                    "processes": result.locks.iter().map(|l| {
                        serde_json::json!({
                            "pid": l.pid,
                            "name": l.name,
                            "command": l.command,
                            "safe_to_kill": l.safe_to_kill,
                            "open_files": l.open_files,
                            "lock_types": l.lock_types.iter()
                                .map(|t| format!("{}", t))
                                .collect::<Vec<_>>(),
                        })
                    }).collect::<Vec<_>>(),
                    "warnings": result.warnings,
                });
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else {
                print!("{}", result);

                if result.has_locks() {
                    println!();
                    if result.has_critical {
                        println!("⚠  Critical system processes detected — cannot safely terminate.");
                        println!("   Unmount the device or close applications manually.");
                    } else {
                        println!("Tip: Close the above applications or unmount the device before writing.");
                    }
                }
            }
        }

        "busy" | "is-busy" => {
            let device = &opts.device;
            let busy = proclock::is_device_busy(device)?;

            if opts.json {
                let json = serde_json::json!({
                    "device": device,
                    "busy": busy,
                });
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else if busy {
                println!("{} is BUSY — processes are holding locks on it.", device);
                std::process::exit(1);
            } else {
                println!("{} is FREE — no process locks detected.", device);
            }
        }

        "report" => {
            let device = &opts.device;
            let report = proclock::lock_report(device)?;
            println!("{}", report);
        }

        other => {
            anyhow::bail!(
                "Unknown proclock action '{}'. Available: scan, busy, report",
                other
            );
        }
    }

    Ok(())
}
