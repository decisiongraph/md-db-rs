use std::path::PathBuf;

use clap::Args;
use md_db::document::Document;

#[derive(Debug, Args)]
pub struct SetArgs {
    /// Path to the markdown file
    pub file: PathBuf,

    /// Set frontmatter fields (repeatable): key=value
    #[arg(long = "field")]
    pub fields: Vec<String>,

    /// Target section heading
    #[arg(long)]
    pub section: Option<String>,

    /// Replace section content with this text
    #[arg(long)]
    pub content: Option<String>,

    /// Append text to section
    #[arg(long)]
    pub append: Option<String>,

    /// Table index within section (0-based)
    #[arg(long)]
    pub table: Option<usize>,

    /// Update table cell: "Column,Row" (use with --value)
    #[arg(long)]
    pub cell: Option<String>,

    /// Value for --cell
    #[arg(long)]
    pub value: Option<String>,

    /// Add a row to a table (comma-separated, use \\, for literal commas)
    #[arg(long = "add-row")]
    pub add_row: Option<String>,

    /// Replace section content in batch (repeatable): "Heading=new content"
    #[arg(long = "section-set")]
    pub section_sets: Vec<String>,

    /// Print result to stdout instead of writing file
    #[arg(long)]
    pub dry_run: bool,
}

pub fn run(args: &SetArgs) -> Result<(), Box<dyn std::error::Error>> {
    let mut doc = Document::from_file(&args.file)?;

    // --field key=value
    for field_str in &args.fields {
        let (key, value) = field_str
            .split_once('=')
            .ok_or_else(|| format!("invalid --field format '{}', expected key=value", field_str))?;
        doc.set_field_from_str(key, value);
    }

    // --section-set batch: "Heading=content"
    for ss in &args.section_sets {
        let (heading, content) = ss
            .split_once('=')
            .ok_or_else(|| format!("invalid --section-set format '{}', expected Heading=content", ss))?;
        doc.replace_section_content(heading.trim(), &format!("{}\n", content.trim()))?;
    }

    // --section operations
    if let Some(ref heading) = args.section {
        // --content: replace section content
        if let Some(ref content) = args.content {
            doc.replace_section_content(heading, &format!("{content}\n"))?;
        }

        // --append: append to section
        if let Some(ref text) = args.append {
            doc.append_to_section(heading, text)?;
        }

        // --table operations
        if let Some(table_idx) = args.table {
            // --cell + --value: update cell
            if let Some(ref cell_spec) = args.cell {
                let value = args
                    .value
                    .as_deref()
                    .ok_or("--cell requires --value")?;
                let (col, row) = parse_cell_spec(cell_spec)?;
                doc.set_table_cell(heading, table_idx, &col, row, value)?;
            }

            // --add-row
            if let Some(ref row_str) = args.add_row {
                let values = parse_row_values(row_str);
                doc.add_table_row(heading, table_idx, values)?;
            }
        }
    }

    if args.dry_run {
        print!("{}", doc.raw);
    } else {
        doc.save()?;
    }

    Ok(())
}

fn parse_cell_spec(spec: &str) -> Result<(String, usize), Box<dyn std::error::Error>> {
    let parts: Vec<&str> = spec.splitn(2, ',').collect();
    if parts.len() != 2 {
        return Err(format!("invalid cell spec '{}', expected 'Column,Row'", spec).into());
    }
    let col = parts[0].to_string();
    let row: usize = parts[1].parse()?;
    Ok((col, row))
}

/// Parse comma-separated row values. Use `\,` for literal commas.
fn parse_row_values(s: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut current = String::new();
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek() == Some(&',') {
            current.push(',');
            chars.next();
        } else if c == ',' {
            values.push(current.trim().to_string());
            current = String::new();
        } else {
            current.push(c);
        }
    }
    values.push(current.trim().to_string());
    values
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_row_values() {
        assert_eq!(parse_row_values("a,b,c"), vec!["a", "b", "c"]);
        assert_eq!(parse_row_values("a\\,b,c"), vec!["a,b", "c"]);
        assert_eq!(parse_row_values("solo"), vec!["solo"]);
    }
}
