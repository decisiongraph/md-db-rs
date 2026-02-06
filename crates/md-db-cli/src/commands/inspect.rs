use std::collections::HashSet;
use std::path::PathBuf;

use clap::Args;
use md_db::document::Document;
use md_db::output::{self, OutputFormat};
use md_db::schema::Schema;
use md_db::users::UserConfig;
use md_db::validation;

#[derive(Debug, Args)]
pub struct InspectArgs {
    /// Path to the markdown file (omit when using --stdin)
    pub file: Option<PathBuf>,

    /// Path to KDL schema file
    #[arg(long)]
    pub schema: PathBuf,

    /// Read document from stdin
    #[arg(long)]
    pub stdin: bool,

    /// Path to user/team config YAML file
    #[arg(long)]
    pub users: Option<PathBuf>,

    /// Output format: json, compact, text, auto (auto=json when piped)
    #[arg(long, default_value = "auto")]
    pub format: String,
}

pub fn run(args: &InspectArgs) -> Result<(), Box<dyn std::error::Error>> {
    let schema = Schema::from_file(&args.schema)?;
    let user_config = match &args.users {
        Some(path) => Some(UserConfig::from_file(path)?),
        None => None,
    };

    let doc = if args.stdin {
        let mut content = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut content)?;
        Document::from_str(&content)?
    } else {
        let file = args
            .file
            .as_ref()
            .ok_or("file argument required when not using --stdin")?;
        Document::from_file(file)?
    };

    let format = OutputFormat::from_str(&args.format).unwrap_or(OutputFormat::auto());

    // Validate
    let file_result = validation::validate_document(
        &doc,
        &schema,
        &HashSet::new(),
        &HashSet::new(),
        user_config.as_ref(),
    );

    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&to_json(&doc, &file_result, &schema))?);
        }
        OutputFormat::Compact => {
            // Compact: frontmatter fields as key=value, then diagnostics
            if let Some(ref fm) = doc.frontmatter {
                for key in fm.keys() {
                    if let Some(val) = fm.get(key) {
                        println!("field:{}={}", key, output::yaml_value_display(val));
                    }
                }
            }
            for s in doc.sections() {
                println!("section:{}:level={}", s.heading.trim(), s.level);
            }
            for d in &file_result.diagnostics {
                println!("diag:{}", d.to_compact());
            }
        }
        _ => {
            // Text
            if let Some(ref fm) = doc.frontmatter {
                println!("Frontmatter:");
                for key in fm.keys() {
                    if let Some(val) = fm.get(key) {
                        println!("  {}: {}", key, output::yaml_value_display(val));
                    }
                }
            }
            println!("\nSections:");
            for s in doc.sections() {
                let hashes = "#".repeat(s.level as usize);
                println!("  {hashes} {}", s.heading.trim());
            }
            if !file_result.diagnostics.is_empty() {
                println!("\nDiagnostics:");
                for d in &file_result.diagnostics {
                    println!("{d}");
                }
            } else {
                println!("\nValid.");
            }
        }
    }

    Ok(())
}

fn to_json(
    doc: &Document,
    file_result: &validation::FileResult,
    schema: &Schema,
) -> serde_json::Value {
    let frontmatter = doc
        .frontmatter
        .as_ref()
        .map(|fm| fm.to_json())
        .unwrap_or(serde_json::Value::Null);

    let sections: Vec<serde_json::Value> = doc
        .sections()
        .iter()
        .map(|s| {
            serde_json::json!({
                "heading": s.heading.trim(),
                "level": s.level,
                "content_length": s.content.len(),
            })
        })
        .collect();

    let diagnostics: Vec<serde_json::Value> = file_result
        .diagnostics
        .iter()
        .map(|d| {
            serde_json::json!({
                "severity": d.severity.to_string(),
                "code": d.code,
                "message": d.message,
                "location": d.location,
                "hint": d.hint,
            })
        })
        .collect();

    // Include applicable type schema info
    let type_name = doc
        .frontmatter
        .as_ref()
        .and_then(|fm| fm.get_display("type"));
    let type_info = type_name
        .as_ref()
        .and_then(|name| schema.get_type(name))
        .map(|td| {
            let fields: Vec<serde_json::Value> = td
                .fields
                .iter()
                .map(|f| {
                    let mut obj = serde_json::json!({
                        "name": f.name,
                        "type": f.field_type.to_string(),
                        "required": f.required,
                    });
                    if let Some(ref d) = f.description {
                        obj["description"] = serde_json::Value::String(d.clone());
                    }
                    if let Some(ref d) = f.default {
                        obj["default"] = serde_json::Value::String(d.clone());
                    }
                    obj
                })
                .collect();
            serde_json::json!({
                "name": td.name,
                "description": td.description,
                "fields": fields,
            })
        });

    serde_json::json!({
        "path": doc.path.as_ref().map(|p| p.display().to_string()),
        "frontmatter": frontmatter,
        "sections": sections,
        "diagnostics": diagnostics,
        "errors": file_result.errors(),
        "warnings": file_result.warnings(),
        "valid": file_result.errors() == 0,
        "schema_type": type_info,
    })
}

