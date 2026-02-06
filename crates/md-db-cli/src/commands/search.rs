use std::path::PathBuf;

use clap::Args;
use md_db::output::OutputFormat;
use md_db::search::{self, SearchOptions};

#[derive(Debug, Args)]
pub struct SearchArgs {
    /// Directory to search
    pub dir: PathBuf,

    /// Search query (substring match)
    pub query: String,

    /// Only search within this section heading
    #[arg(long)]
    pub section: Option<String>,

    /// Only search within this frontmatter field
    #[arg(long)]
    pub field: Option<String>,

    /// Case-sensitive search (default: case-insensitive)
    #[arg(long)]
    pub case_sensitive: bool,

    /// Maximum number of documents to return
    #[arg(long)]
    pub max_results: Option<usize>,

    /// Output format: text, json
    #[arg(long, default_value = "text")]
    pub format: String,
}

pub fn run(args: &SearchArgs) -> Result<(), Box<dyn std::error::Error>> {
    let format = OutputFormat::from_str(&args.format).unwrap_or(OutputFormat::Text);

    let options = SearchOptions {
        case_sensitive: args.case_sensitive,
        section_filter: args.section.clone(),
        field_filter: args.field.clone(),
        max_results: args.max_results,
    };

    let results = search::search_documents(&args.dir, &args.query, &options)?;

    match format {
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(&results)?;
            println!("{json}");
        }
        _ => {
            if results.is_empty() {
                println!("No matches found.");
                return Ok(());
            }
            for result in &results {
                for m in &result.matches {
                    println!(
                        "{}:{}:{}: {}",
                        result.path, m.section, m.line, m.context
                    );
                }
            }
        }
    }

    Ok(())
}
