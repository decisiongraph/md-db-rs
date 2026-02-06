use std::path::PathBuf;

use clap::Args;
use markdown_all::document::Document;
use markdown_all::error::Error;
use markdown_all::output::{self, OutputFormat};

#[derive(Debug, Args)]
pub struct GetArgs {
    /// Path to the markdown file
    pub file: PathBuf,

    /// Get a frontmatter field by key (supports dotted paths like "links.ref")
    #[arg(long)]
    pub field: Option<String>,

    /// Get the full frontmatter
    #[arg(long)]
    pub frontmatter: bool,

    /// Get a section by heading text
    #[arg(long)]
    pub section: Option<String>,

    /// Get a table by index within the section (0-based)
    #[arg(long)]
    pub table: Option<usize>,

    /// Get a single cell: "Column,Row" (row is 0-based)
    #[arg(long)]
    pub cell: Option<String>,

    /// Output format: text, markdown, json
    #[arg(long, default_value = "markdown")]
    pub format: String,
}

pub fn run(args: &GetArgs) -> Result<(), Box<dyn std::error::Error>> {
    let doc = Document::from_file(&args.file)?;
    let format = OutputFormat::from_str(&args.format).unwrap_or(OutputFormat::Markdown);

    // --field: return bare frontmatter value
    if let Some(ref field) = args.field {
        let fm = doc.frontmatter()?;
        let val = fm.get(field).ok_or(Error::FieldNotFound(field.clone()))?;
        println!("{}", output::format_field_value(val, format));
        return Ok(());
    }

    // --frontmatter: return full frontmatter
    if args.frontmatter {
        let fm = doc.frontmatter()?;
        match format {
            OutputFormat::Json => {
                println!("{}", serde_json::to_string_pretty(&fm.to_json())?);
            }
            _ => {
                println!("{}", fm.to_yaml()?);
            }
        }
        return Ok(());
    }

    // --section: get section content
    if let Some(ref heading) = args.section {
        let section = doc.get_section(heading)?;

        // --table within section
        if let Some(table_idx) = args.table {
            let tables = section.tables();
            let table = tables
                .get(table_idx)
                .ok_or(Error::TableNotFound(table_idx))?;

            // --cell within table
            if let Some(ref cell_spec) = args.cell {
                let (col, row) = parse_cell_spec(cell_spec)?;
                let val = table.get_cell_or_err(&col, row)?;
                println!("{val}");
                return Ok(());
            }

            println!("{}", output::format_table(table, format));
            return Ok(());
        }

        // Section content
        match format {
            OutputFormat::Text => println!("{}", section.text()),
            OutputFormat::Json => {
                let json = serde_json::json!({
                    "heading": section.heading,
                    "level": section.level,
                    "content": section.content,
                });
                println!("{}", serde_json::to_string_pretty(&json)?);
            }
            OutputFormat::Markdown => print!("{}", section.raw),
        }
        return Ok(());
    }

    // No specific option: output entire document
    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&doc.to_json())?);
        }
        _ => {
            print!("{}", doc.body);
        }
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
