// Man page generation — produces roff-format man pages from clap definitions.
//
// Usage:
//   abt man --output-dir /usr/local/share/man/man1/
//   abt man   # writes to current directory

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use clap::CommandFactory;

use crate::cli::{Args, ManOpts};

pub fn execute(opts: ManOpts) -> Result<()> {
    let output_dir = Path::new(&opts.output_dir);
    fs::create_dir_all(output_dir)
        .with_context(|| format!("Failed to create output directory: {}", output_dir.display()))?;

    let cmd = Args::command();

    // Generate the main man page
    let man = clap_mangen::Man::new(cmd.clone());
    let mut buf = Vec::new();
    man.render(&mut buf)?;
    let main_path = output_dir.join("abt.1");
    fs::write(&main_path, &buf)
        .with_context(|| format!("Failed to write {}", main_path.display()))?;
    log::info!("Generated {}", main_path.display());

    // Generate a man page for each subcommand
    let mut count = 1;
    for sub in cmd.get_subcommands() {
        let name = sub.get_name().to_string();
        let sub_cmd = sub.clone();
        let man = clap_mangen::Man::new(sub_cmd);
        let mut buf = Vec::new();
        man.render(&mut buf)?;
        let sub_path = output_dir.join(format!("abt-{}.1", name));
        fs::write(&sub_path, &buf)
            .with_context(|| format!("Failed to write {}", sub_path.display()))?;
        log::info!("Generated {}", sub_path.display());
        count += 1;
    }

    println!("Generated {} man pages in {}", count, output_dir.display());
    Ok(())
}
