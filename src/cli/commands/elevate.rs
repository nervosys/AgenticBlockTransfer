use anyhow::Result;

use crate::cli::ElevateOpts;
use crate::core::elevate;

pub async fn execute(opts: ElevateOpts) -> Result<()> {
    match opts.action.as_str() {
        "status" | "check" => {
            let status = elevate::status();

            if opts.json {
                let json = serde_json::json!({
                    "is_elevated": status.is_elevated,
                    "username": status.username,
                    "elevation_needed": status.elevation_needed,
                    "preferred_method": format!("{}", status.preferred_method),
                    "available_methods": status.available_methods.iter()
                        .map(|m| format!("{}", m))
                        .collect::<Vec<_>>(),
                });
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else {
                println!("{}", status);
            }
        }

        "run" | "elevate" => {
            if elevate::is_elevated() {
                if opts.json {
                    let json = serde_json::json!({
                        "status": "already_elevated",
                        "success": true,
                    });
                    println!("{}", serde_json::to_string_pretty(&json)?);
                } else {
                    println!("Already running with elevated privileges.");
                }
                return Ok(());
            }

            println!("Requesting elevated privileges...");
            let result = if let Some(ref method) = opts.method {
                let m = match method.as_str() {
                    "uac" => elevate::ElevationMethod::Uac,
                    "pkexec" => elevate::ElevationMethod::Pkexec,
                    "sudo" => elevate::ElevationMethod::Sudo,
                    "osascript" => elevate::ElevationMethod::Osascript,
                    other => anyhow::bail!("Unknown elevation method '{}'. Available: uac, pkexec, sudo, osascript", other),
                };
                elevate::elevate_with(m)?
            } else {
                elevate::elevate()?
            };

            if opts.json {
                let json = serde_json::json!({
                    "success": result.success,
                    "method": format!("{}", result.method),
                    "error": result.error,
                    "should_exit": result.should_exit,
                });
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else {
                println!("{}", result);
            }

            if result.should_exit {
                std::process::exit(0);
            }
        }

        "methods" | "list" => {
            let methods = elevate::available_methods();

            if opts.json {
                let json = serde_json::json!({
                    "methods": methods.iter()
                        .map(|m| format!("{}", m))
                        .collect::<Vec<_>>(),
                    "preferred": format!("{}", elevate::preferred_method()),
                });
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else {
                println!("Available elevation methods:");
                let preferred = elevate::preferred_method();
                for m in &methods {
                    let marker = if *m == preferred { " (preferred)" } else { "" };
                    println!("  • {}{}", m, marker);
                }
            }
        }

        other => {
            anyhow::bail!(
                "Unknown elevate action '{}'. Available: status, run, methods",
                other
            );
        }
    }

    Ok(())
}
