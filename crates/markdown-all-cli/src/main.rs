use clap::{Parser, Subcommand};

mod commands;

#[derive(Debug, Parser)]
#[command(name = "markdown-all", about = "Markdown-as-Database CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Read fields, sections, or table cells from a markdown file
    Get(commands::get::GetArgs),
    /// List and filter markdown files by frontmatter
    List(commands::list::ListArgs),
}

fn main() {
    let cli = Cli::parse();

    let result = match &cli.command {
        Commands::Get(args) => commands::get::run(args),
        Commands::List(args) => commands::list::run(args),
    };

    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
