use clap::{CommandFactory, Parser};
use clap_complete::{generate, Shell};

mod commands;

#[derive(Debug, Parser)]
#[command(name = "md-db", about = "Markdown-as-Database CLI")]
struct Cli {
    #[command(subcommand)]
    command: CliCommand,
}

#[derive(Debug, clap::Subcommand)]
enum CliCommand {
    #[command(flatten)]
    App(commands::Commands),
    /// Generate shell completions for bash, zsh, fish, elvish, or powershell
    Completions {
        /// Shell to generate completions for
        shell: Shell,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        CliCommand::Completions { shell } => {
            let mut cmd = Cli::command();
            generate(shell, &mut cmd, "md-db", &mut std::io::stdout());
        }
        CliCommand::App(ref cmd) => {
            if let Err(e) = commands::run(cmd) {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
    }
}
