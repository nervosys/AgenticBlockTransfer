// CLI command: watchdog — Configure and test write watchdog settings

use anyhow::Result;
use std::sync::Arc;

use crate::cli::WatchdogOpts;
use crate::core::watchdog;

pub async fn execute(opts: WatchdogOpts) -> Result<()> {
    match opts.action.as_str() {
        "show" | "config" => {
            let config = match opts.preset.as_deref() {
                Some("lenient") => watchdog::WatchdogConfig::lenient(),
                Some("strict") => watchdog::WatchdogConfig::strict(),
                _ => watchdog::WatchdogConfig::default(),
            };

            if opts.json {
                println!("{}", serde_json::to_string_pretty(&config)?);
            } else {
                println!("Watchdog Configuration ({})", opts.preset.as_deref().unwrap_or("default"));
                println!("  Check interval:    {:?}", config.check_interval);
                println!("  Stall timeout:     {:?}", config.stall_timeout);
                println!("  Max stalls:        {}", config.max_stalls);
                println!("  Min queue depth:   {}", config.min_queue_depth);
                println!("  Queue depth factor:{}", config.queue_depth_factor);
                println!("  Enabled:           {}", config.enabled);
                println!("  Escalation chain:");
                for (i, action) in config.escalation.iter().enumerate() {
                    println!("    {}: {}", i + 1, action);
                }
            }
        }
        "test" | "simulate" => {
            println!("Simulating watchdog with stall scenario...\n");
            let config = watchdog::WatchdogConfig::default();
            let state = Arc::new(watchdog::WatchdogState::new(1_000_000, 8));
            let mut wd = watchdog::WriteWatchdog::new(config, state.clone())?;

            // Simulate: progress, then stall, then progress, then verify recovery.
            println!("Phase 1: Normal progress");
            state.update_progress(100_000);
            let action = wd.check(0);
            println!("  Check: progress 0 → 100000, action = {:?}", action);

            println!("\nPhase 2: Stall (no progress)");
            for i in 1..=4 {
                let bytes = state.get_bytes_written();
                let action = wd.check(bytes);
                println!(
                    "  Check {}: bytes={}, action={:?}, qd={}",
                    i,
                    bytes,
                    action,
                    state.get_queue_depth()
                );
            }

            println!("\nPhase 3: Recovery (progress resumes)");
            state.update_progress(500_000);
            let action = wd.check(100_000);
            println!("  Check: progress resumed, action = {:?}", action);

            let summary = wd.summary();
            println!("\n{}", watchdog::format_summary(&summary));

            if opts.json {
                println!("\n{}", serde_json::to_string_pretty(&summary)?);
            }
        }
        other => {
            anyhow::bail!(
                "Unknown action: '{}'. Use 'show', 'config', 'test', or 'simulate'.",
                other
            );
        }
    }

    Ok(())
}
