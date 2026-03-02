// CLI command: restore — Restore a drive to factory-clean state

use anyhow::Result;

use crate::cli::RestoreOpts;
use crate::core::restore;

pub async fn execute(opts: RestoreOpts) -> Result<()> {
    let table_type: restore::PartitionTableType = opts.table_type.parse()?;
    let filesystem: restore::RestoreFilesystem = opts.filesystem.parse()?;

    let config = restore::RestoreConfig {
        device: opts.device.clone(),
        table_type,
        filesystem,
        label: opts.label.clone(),
        wipe_table: !opts.no_wipe,
        quick: !opts.full,
        force: opts.force,
    };

    match opts.action.as_str() {
        "plan" => {
            let plan = restore::plan_restore(&config)?;
            if opts.json {
                println!("{}", serde_json::to_string_pretty(&plan)?);
            } else {
                println!("Drive Restore Plan");
                println!("  Device:     {}", plan.device);
                println!("  Capacity:   {}", plan.capacity_human);
                println!("  Table:      {}", plan.table_type);
                println!("  Filesystem: {}", plan.filesystem);
                println!("  Label:      {}", plan.label);
                println!("  Wipe:       {}", if plan.will_wipe { "Yes" } else { "No" });
                println!();
                if !plan.warnings.is_empty() {
                    println!("Warnings:");
                    for w in &plan.warnings {
                        println!("  ⚠ {}", w);
                    }
                    println!();
                }
                if !plan.commands.is_empty() {
                    println!("Commands to execute:");
                    for cmd in &plan.commands {
                        println!("  $ {}", cmd);
                    }
                }
            }
        }
        "execute" | "run" => {
            if !opts.force {
                println!("⚠ WARNING: This will DESTROY ALL DATA on {}", opts.device);
                println!("  Use --force to proceed without confirmation.");
                return Ok(());
            }
            let result = restore::execute_restore(&config).await?;
            if opts.json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("Drive restored successfully!");
                println!("  Device:     {}", result.device);
                println!("  Table:      {}", result.table_type);
                println!("  Filesystem: {}", result.filesystem);
                println!("  Label:      {}", result.label);
                println!("  Duration:   {:.2}s", result.duration_seconds);
                println!();
                println!("Steps:");
                for step in &result.steps {
                    println!("  [{}] {} ({} ms)", step.status, step.name, step.duration_ms);
                    if let Some(ref detail) = step.detail {
                        println!("       {}", detail);
                    }
                }
            }
        }
        other => {
            anyhow::bail!(
                "Unknown action: '{}'. Use 'plan' or 'execute'.",
                other
            );
        }
    }

    Ok(())
}
