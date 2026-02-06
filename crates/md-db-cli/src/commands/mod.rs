use clap::Subcommand;

pub mod batch;
pub mod deprecate;
pub mod diff;
pub mod describe;
pub mod export;
pub mod fix;
pub mod get;
pub mod graph;
pub mod hook;
pub mod init;
pub mod inspect;
pub mod list;
pub mod mcp;
pub mod migrate;
pub mod new;
pub mod refs;
pub mod rename;
pub mod search;
pub mod set;
pub mod stats;
pub mod sync;
pub mod validate;
pub mod watch;

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Apply field mutations to all docs matching a filter
    Batch(batch::BatchArgs),
    /// Deprecate a document (set status, optionally mark superseded)
    Deprecate(deprecate::DeprecateArgs),
    /// Show structural diff between two versions of a document
    Diff(diff::DiffArgs),
    /// Describe schema types, fields, sections, and relations
    Describe(describe::DescribeArgs),
    /// Export documents to a static HTML site
    Export(export::ExportArgs),
    /// Auto-fix common validation errors
    Fix(fix::FixArgs),
    /// Read fields, sections, or table cells from a markdown file
    Get(get::GetArgs),
    /// Export the document link graph as mermaid, DOT, or JSON
    Graph(graph::GraphArgs),
    /// Install or uninstall a git pre-commit hook
    Hook(hook::HookArgs),
    /// Scaffold a new md-db project with schema.kdl and directory structure
    Init(init::InitArgs),
    /// Inspect a document: frontmatter + sections + validation in one call
    Inspect(inspect::InspectArgs),
    /// List and filter markdown files by frontmatter
    List(list::ListArgs),
    /// Start MCP (Model Context Protocol) server over stdio
    Mcp,
    /// Detect schema changes and migrate documents
    Migrate(migrate::MigrateArgs),
    /// Validate markdown files against a KDL schema
    Validate(validate::ValidateArgs),
    /// Create a new document from a schema type definition
    New(new::NewArgs),
    /// Show forward refs or backlinks for a document
    Refs(refs::RefsArgs),
    /// Rename a document ID and cascade-update all references
    Rename(rename::RenameArgs),
    /// Full-text search across document content and frontmatter
    Search(search::SearchArgs),
    /// Update fields, sections, or table cells in a markdown file
    Set(set::SetArgs),
    /// Show document set health overview (counts, validation, graph stats)
    Stats(stats::StatsArgs),
    /// Sync bidirectional relations (add missing inverse refs)
    Sync(sync::SyncArgs),
    /// Watch directory and re-validate on file changes
    Watch(watch::WatchArgs),
}

/// Run the given command.
pub fn run(command: &Commands) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        Commands::Batch(args) => batch::run(args),
        Commands::Deprecate(args) => deprecate::run(args),
        Commands::Diff(args) => diff::run(args),
        Commands::Describe(args) => describe::run(args),
        Commands::Export(args) => export::run(args),
        Commands::Fix(args) => fix::run(args),
        Commands::Get(args) => get::run(args),
        Commands::Graph(args) => graph::run(args),
        Commands::Hook(args) => hook::run(args),
        Commands::Init(args) => init::run(args),
        Commands::Inspect(args) => inspect::run(args),
        Commands::List(args) => list::run(args),
        Commands::Mcp => mcp::run(),
        Commands::Migrate(args) => migrate::run(args),
        Commands::Validate(args) => validate::run(args),
        Commands::New(args) => new::run(args),
        Commands::Refs(args) => refs::run(args),
        Commands::Rename(args) => rename::run(args),
        Commands::Search(args) => search::run(args),
        Commands::Set(args) => set::run(args),
        Commands::Stats(args) => stats::run(args),
        Commands::Sync(args) => sync::run(args),
        Commands::Watch(args) => watch::run(args),
    }
}
