use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};

mod commands;

#[derive(Debug, Parser)]
#[command(name = "md-db", about = "Markdown-as-Database CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Deprecate a document (set status, optionally mark superseded)
    Deprecate(commands::deprecate::DeprecateArgs),
    /// Show structural diff between two versions of a document
    Diff(commands::diff::DiffArgs),
    /// Describe schema types, fields, sections, and relations
    Describe(commands::describe::DescribeArgs),
    /// Read fields, sections, or table cells from a markdown file
    Get(commands::get::GetArgs),
    /// Export the document link graph as mermaid, DOT, or JSON
    Graph(commands::graph::GraphArgs),
    /// Inspect a document: frontmatter + sections + validation in one call
    Inspect(commands::inspect::InspectArgs),
    /// List and filter markdown files by frontmatter
    List(commands::list::ListArgs),
    /// Validate markdown files against a KDL schema
    Validate(commands::validate::ValidateArgs),
    /// Create a new document from a schema type definition
    New(commands::new::NewArgs),
    /// Show forward refs or backlinks for a document
    Refs(commands::refs::RefsArgs),
    /// Full-text search across document content and frontmatter
    Search(commands::search::SearchArgs),
    /// Update fields, sections, or table cells in a markdown file
    Set(commands::set::SetArgs),
    /// Generate shell completions for bash, zsh, fish, elvish, or powershell
    Completions {
        /// Shell to generate completions for
        shell: Shell,
    },
}

fn main() {
    let cli = Cli::parse();

    let result = match &cli.command {
        Commands::Deprecate(args) => commands::deprecate::run(args),
        Commands::Diff(args) => commands::diff::run(args),
        Commands::Describe(args) => commands::describe::run(args),
        Commands::Get(args) => commands::get::run(args),
        Commands::Graph(args) => commands::graph::run(args),
        Commands::Inspect(args) => commands::inspect::run(args),
        Commands::List(args) => commands::list::run(args),
        Commands::Validate(args) => commands::validate::run(args),
        Commands::New(args) => commands::new::run(args),
        Commands::Refs(args) => commands::refs::run(args),
        Commands::Search(args) => commands::search::run(args),
        Commands::Set(args) => commands::set::run(args),
        Commands::Completions { shell } => {
            let mut cmd = Cli::command();
            generate(*shell, &mut cmd, "md-db", &mut std::io::stdout());
            return;
        }
    };

    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
