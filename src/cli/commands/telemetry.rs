// CLI command: telemetry — View and export performance telemetry data

use anyhow::Result;
use std::path::Path;

use crate::cli::TelemetryOpts;
use crate::core::telemetry;

pub async fn execute(opts: TelemetryOpts) -> Result<()> {
    match opts.action.as_str() {
        "show" | "view" => {
            let path = opts
                .file
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("--file is required for show/view"))?;
            let report = telemetry::load_report(Path::new(path))?;
            if opts.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("{}", telemetry::summarize_report(&report));
            }
        }
        "demo" => {
            // Create a demo telemetry session to show the output format.
            let mut session = telemetry::TelemetrySession::new("demo-write");
            session.record_phase("download", 50_000_000, 0.5);
            session.record_phase("decompress", 50_000_000, 0.3);
            session.record_phase("write", 50_000_000, 1.2);
            session.record_phase("verify", 50_000_000, 0.8);
            session.record_event(
                telemetry::EventType::BufferStarvation,
                Some("ring buffer empty for 50ms"),
            );
            session.detect_bottleneck();
            let report = session.finalize(true, None);

            if opts.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("{}", telemetry::summarize_report(&report));
            }

            // Export if output path provided
            if let Some(ref out) = opts.output {
                telemetry::export_report(&report, Path::new(out))?;
                println!("\nExported to: {}", out);
            }
        }
        "export" => {
            let input = opts
                .file
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("--file is required for export"))?;
            let output = opts
                .output
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("--output is required for export"))?;

            let report = telemetry::load_report(Path::new(input))?;
            telemetry::export_report(&report, Path::new(output))?;
            println!("Exported {} → {}", input, output);
        }
        other => {
            anyhow::bail!(
                "Unknown action: '{}'. Use 'show', 'demo', or 'export'.",
                other
            );
        }
    }

    Ok(())
}
