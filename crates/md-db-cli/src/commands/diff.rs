use std::path::PathBuf;

use clap::Args;
use md_db::diff::{self, FieldChangeKind, SectionChangeKind};
use md_db::document::Document;
use md_db::output::OutputFormat;

#[derive(Debug, Args)]
pub struct DiffArgs {
    /// Old version of the markdown file
    pub old: PathBuf,

    /// New version of the markdown file (omit to read from stdin)
    pub new: Option<PathBuf>,

    /// Read new version from stdin instead of a file
    #[arg(long)]
    pub stdin: bool,

    /// Output format: text, json
    #[arg(long, default_value = "text")]
    pub format: String,
}

pub fn run(args: &DiffArgs) -> Result<(), Box<dyn std::error::Error>> {
    let old_doc = Document::from_file(&args.old)?;

    let new_content = if args.stdin {
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)?;
        buf
    } else {
        let new_path = args
            .new
            .as_ref()
            .ok_or("second file argument required when not using --stdin")?;
        std::fs::read_to_string(new_path)?
    };

    let mut result = diff::diff_documents(&old_doc.raw, &new_content)?;

    // Attach path from old file
    result.path = Some(args.old.display().to_string());

    let format = OutputFormat::from_str(&args.format).unwrap_or(OutputFormat::Text);

    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        _ => {
            print_text(&result);
        }
    }

    Ok(())
}

fn print_text(diff: &diff::DocDiff) {
    // Header line
    let header = match (&diff.path, &diff.id) {
        (Some(p), Some(id)) => format!("{p} ({id}):"),
        (Some(p), None) => format!("{p}:"),
        (None, Some(id)) => format!("({id}):"),
        (None, None) => "document:".to_string(),
    };
    println!("{header}");

    if diff.is_empty() {
        println!("  no structural changes");
        return;
    }

    for fc in &diff.field_changes {
        match fc.kind {
            FieldChangeKind::Added => {
                println!(
                    "  + field added: {}: {}",
                    fc.field,
                    fc.new.as_deref().unwrap_or("null")
                );
            }
            FieldChangeKind::Removed => {
                println!(
                    "  - field removed: {}: {}",
                    fc.field,
                    fc.old.as_deref().unwrap_or("null")
                );
            }
            FieldChangeKind::Changed => {
                println!(
                    "  ~ field changed: {}: {} \u{2192} {}",
                    fc.field,
                    fc.old.as_deref().unwrap_or("null"),
                    fc.new.as_deref().unwrap_or("null")
                );
            }
        }
    }

    for sc in &diff.section_changes {
        match sc.kind {
            SectionChangeKind::Added => {
                println!("  + section added: {}", sc.section);
            }
            SectionChangeKind::Removed => {
                println!("  - section removed: {}", sc.section);
            }
            SectionChangeKind::Modified => {
                let detail = match (sc.lines_added, sc.lines_removed) {
                    (Some(a), Some(r)) => format!(" (+{a} -{r} lines)"),
                    (Some(a), None) => format!(" (+{a} lines)"),
                    (None, Some(r)) => format!(" (-{r} lines)"),
                    (None, None) => String::new(),
                };
                println!("  ~ section modified: {}{detail}", sc.section);
            }
        }
    }
}
