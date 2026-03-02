// Shell completion generation — bash, zsh, fish, PowerShell
//
// Usage:
//   abt completions bash   > /etc/bash_completion.d/abt
//   abt completions zsh    > /usr/local/share/zsh/site-functions/_abt
//   abt completions fish   > ~/.config/fish/completions/abt.fish
//   abt completions powershell | Out-String | Invoke-Expression

use anyhow::Result;

use crate::cli::{Args, CompletionsOpts};

pub fn execute(opts: CompletionsOpts) -> Result<()> {
    let mut cmd = <Args as clap::CommandFactory>::command();
    clap_complete::generate(
        opts.shell,
        &mut cmd,
        "abt",
        &mut std::io::stdout(),
    );
    Ok(())
}
