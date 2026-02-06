use std::path::PathBuf;

use clap::Args;
use md_db::discovery::{self, Filter};
use md_db::frontmatter::Frontmatter;
use md_db::output::{self, ListEntry, OutputFormat};

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

    /// Filter by field NOT equal: key!=value (repeatable)
    #[arg(long = "not-field", value_name = "KEY!=VALUE")]
    pub not_fields: Vec<String>,

    /// Filter by field containing substring: key~=value (repeatable)
    #[arg(long = "contains", value_name = "KEY~=VALUE")]
    pub contains_fields: Vec<String>,

    /// Filter by field in set: key=val1,val2,val3 (repeatable)
    #[arg(long = "in", value_name = "KEY=VAL1,VAL2")]
    pub in_fields: Vec<String>,

    /// Filter by field existence (repeatable)
    #[arg(long = "has-field", value_name = "KEY")]
    pub has_fields: Vec<String>,

    /// Filter by field absence (repeatable)
    #[arg(long = "not-has-field", value_name = "KEY")]
    pub not_has_fields: Vec<String>,

    /// Sort by frontmatter field (prefix with - for descending, e.g. -date)
    #[arg(long)]
    pub sort: Option<String>,

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
    for f in &args.not_fields {
        if let Some((key, value)) = f.split_once("!=") {
            filters.push(Filter::FieldNotEquals {
                key: key.to_string(),
                value: value.to_string(),
            });
        } else if let Some((key, value)) = f.split_once('=') {
            // Also accept key=value format for --not-field
            filters.push(Filter::FieldNotEquals {
                key: key.to_string(),
                value: value.to_string(),
            });
        }
    }
    for f in &args.contains_fields {
        if let Some((key, value)) = f.split_once("~=") {
            filters.push(Filter::FieldContains {
                key: key.to_string(),
                value: value.to_string(),
            });
        } else if let Some((key, value)) = f.split_once('=') {
            filters.push(Filter::FieldContains {
                key: key.to_string(),
                value: value.to_string(),
            });
        }
    }
    for f in &args.in_fields {
        if let Some((key, values_str)) = f.split_once('=') {
            let values: Vec<String> = values_str.split(',').map(|s| s.trim().to_string()).collect();
            filters.push(Filter::FieldIn {
                key: key.to_string(),
                values,
            });
        }
    }
    for f in &args.has_fields {
        filters.push(Filter::HasField(f.clone()));
    }
    for f in &args.not_has_fields {
        filters.push(Filter::NotHasField(f.clone()));
    }

    let pattern = args.pattern.as_deref();
    let mut files = discovery::discover_files(&args.dir, pattern, &filters, false)?;

    // Sort by frontmatter field if requested
    if let Some(ref sort_spec) = args.sort {
        let (sort_key, descending) = if let Some(key) = sort_spec.strip_prefix('-') {
            (key, true)
        } else {
            (sort_spec.as_str(), false)
        };

        // Parse frontmatter for all files and sort
        let mut file_vals: Vec<(PathBuf, Option<String>)> = files
            .into_iter()
            .map(|path| {
                let val = std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|content| Frontmatter::try_parse(&content).ok())
                    .and_then(|(fm, _)| fm)
                    .and_then(|fm| fm.get_display(sort_key));
                (path, val)
            })
            .collect();

        file_vals.sort_by(|a, b| {
            let cmp = a.1.as_deref().unwrap_or("").cmp(b.1.as_deref().unwrap_or(""));
            if descending { cmp.reverse() } else { cmp }
        });

        files = file_vals.into_iter().map(|(path, _)| path).collect();
    }

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
