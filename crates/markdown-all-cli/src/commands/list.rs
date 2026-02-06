use std::path::PathBuf;

use clap::Args;
use markdown_all::discovery::{self, Filter};
use markdown_all::frontmatter::Frontmatter;
use markdown_all::output::{self, ListEntry, OutputFormat};

#[derive(Debug, Args)]
pub struct ListArgs {
    /// Directory to search
    pub dir: PathBuf,

    /// Glob pattern for filenames (default: "*.md")
    #[arg(long)]
    pub pattern: Option<String>,

    /// Filter by frontmatter field: key=value (repeatable)
    #[arg(long = "field", value_name = "KEY=VALUE")]
    pub fields: Vec<String>,

    /// Filter by field existence (repeatable)
    #[arg(long = "has-field", value_name = "KEY")]
    pub has_fields: Vec<String>,

    /// Output format: text, json
    #[arg(long, default_value = "text")]
    pub format: String,

    /// Fields to include in JSON output (comma-separated)
    #[arg(long = "fields", value_name = "FIELDS")]
    pub output_fields: Option<String>,
}

pub fn run(args: &ListArgs) -> Result<(), Box<dyn std::error::Error>> {
    let format = OutputFormat::from_str(&args.format).unwrap_or(OutputFormat::Text);

    let mut filters = Vec::new();
    for f in &args.fields {
        if let Some((key, value)) = f.split_once('=') {
            filters.push(Filter::FieldEquals {
                key: key.to_string(),
                value: value.to_string(),
            });
        }
    }
    for f in &args.has_fields {
        filters.push(Filter::HasField(f.clone()));
    }

    let pattern = args.pattern.as_deref();
    let files = discovery::discover_files(&args.dir, pattern, &filters)?;

    let selected_fields: Option<Vec<String>> = args
        .output_fields
        .as_ref()
        .map(|s| s.split(',').map(|f| f.trim().to_string()).collect());

    let entries: Vec<ListEntry> = files
        .iter()
        .map(|path| {
            let fm_json = if format == OutputFormat::Json {
                std::fs::read_to_string(path)
                    .ok()
                    .and_then(|content| Frontmatter::try_parse(&content).ok())
                    .and_then(|(fm, _)| fm.map(|f| f.to_json()))
            } else {
                None
            };
            ListEntry {
                path: path.display().to_string(),
                frontmatter_json: fm_json,
            }
        })
        .collect();

    println!(
        "{}",
        output::format_list(&entries, format, &selected_fields)
    );

    Ok(())
}
