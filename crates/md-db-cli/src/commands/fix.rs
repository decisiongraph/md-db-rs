use std::collections::HashSet;
use std::path::PathBuf;

use clap::Args;
use md_db::document::Document;
use md_db::output::OutputFormat;
use md_db::schema::{FieldType, Schema, TypeDef};
use md_db::template;
use md_db::users::UserConfig;
use md_db::validation;

#[derive(Debug, Args)]
pub struct FixArgs {
    /// Directory or file to fix
    pub dir: PathBuf,

    /// Path to KDL schema file
    #[arg(long)]
    pub schema: PathBuf,

    /// Path to user/team config YAML file
    #[arg(long)]
    pub users: Option<PathBuf>,

    /// Show what would be fixed without writing
    #[arg(long)]
    pub dry_run: bool,

    /// Output format: text, json, compact, auto
    #[arg(long, default_value = "auto")]
    pub format: String,
}

/// A single applied (or skipped) fix action.
#[derive(Debug)]
struct FixAction {
    code: String,
    description: String,
    applied: bool,
}

pub fn run(args: &FixArgs) -> Result<(), Box<dyn std::error::Error>> {
    let schema = Schema::from_file(&args.schema)?;
    let user_config = match &args.users {
        Some(path) => Some(UserConfig::from_file(path)?),
        None => None,
    };

    // Validate to discover diagnostics
    let result = if args.dir.is_file() {
        let doc = Document::from_file(&args.dir)?;
        let fr = validation::validate_document(
            &doc,
            &schema,
            &HashSet::new(),
            &HashSet::new(),
            user_config.as_ref(),
        );
        validation::ValidationResult {
            file_results: vec![fr],
        }
    } else {
        validation::validate_directory(&args.dir, &schema, None, user_config.as_ref())?
    };

    let format = OutputFormat::from_str(&args.format).unwrap_or(OutputFormat::Text);

    let mut total_fixed = 0usize;
    let mut total_skipped = 0usize;
    let mut file_reports: Vec<serde_json::Value> = Vec::new();

    for fr in &result.file_results {
        if fr.diagnostics.is_empty() {
            continue;
        }

        let path = PathBuf::from(&fr.path);
        let mut doc = match Document::from_file(&path) {
            Ok(d) => d,
            Err(_) => continue,
        };

        // Determine document type
        let type_name = match doc
            .frontmatter
            .as_ref()
            .and_then(|fm| fm.get_display("type"))
        {
            Some(t) => t,
            None => continue,
        };
        let type_def = match schema.get_type(&type_name) {
            Some(t) => t,
            None => continue,
        };

        let mut actions: Vec<FixAction> = Vec::new();
        let mut modified = false;

        for diag in &fr.diagnostics {
            match diag.code.as_str() {
                "F010" => {
                    // Missing required field — try to add with default
                    if let Some(action) = fix_missing_field(&mut doc, diag, type_def) {
                        if action.applied {
                            modified = true;
                        }
                        actions.push(action);
                    }
                }
                "F021" => {
                    // Invalid enum value — suggest closest
                    if let Some(action) = fix_invalid_enum(&mut doc, diag, type_def) {
                        if action.applied {
                            modified = true;
                        }
                        actions.push(action);
                    }
                }
                "S010" => {
                    // Missing required section — append heading
                    if let Some(action) = fix_missing_section(&mut doc, diag) {
                        if action.applied {
                            modified = true;
                        }
                        actions.push(action);
                    }
                }
                _ => {} // non-fixable
            }
        }

        if actions.is_empty() {
            continue;
        }

        let fixed_count = actions.iter().filter(|a| a.applied).count();
        let skipped_count = actions.iter().filter(|a| !a.applied).count();
        total_fixed += fixed_count;
        total_skipped += skipped_count;

        // Write back unless dry-run
        if modified && !args.dry_run {
            doc.save()?;
        }

        match format {
            OutputFormat::Json => {
                let acts: Vec<serde_json::Value> = actions
                    .iter()
                    .map(|a| {
                        serde_json::json!({
                            "code": a.code,
                            "description": a.description,
                            "applied": a.applied,
                        })
                    })
                    .collect();
                file_reports.push(serde_json::json!({
                    "path": fr.path,
                    "actions": acts,
                }));
            }
            _ => {
                let dry = if args.dry_run { " (dry-run)" } else { "" };
                println!("{}:{dry}", fr.path);
                for a in &actions {
                    let prefix = if a.applied { "  fixed" } else { "  skipped" };
                    println!("{prefix} {}: {}", a.code, a.description);
                }
                println!();
            }
        }
    }

    match format {
        OutputFormat::Json => {
            let report = serde_json::json!({
                "files": file_reports,
                "fixed": total_fixed,
                "skipped": total_skipped,
                "dry_run": args.dry_run,
            });
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        _ => {
            println!(
                "{total_fixed} fix(es) applied, {total_skipped} skipped{}",
                if args.dry_run { " (dry-run)" } else { "" }
            );
        }
    }

    Ok(())
}

/// Fix F010: missing required field. Add with schema default if available.
fn fix_missing_field(
    doc: &mut Document,
    diag: &validation::Diagnostic,
    type_def: &TypeDef,
) -> Option<FixAction> {
    // Extract field name from message: `missing required field "NAME"`
    let field_name = extract_quoted(&diag.message)?;

    let field_def = type_def.fields.iter().find(|f| f.name == field_name)?;

    match template::field_default_string(field_def) {
        Some(default_val) => {
            doc.set_field_from_str(&field_name, &default_val);
            Some(FixAction {
                code: "F010".into(),
                description: format!(
                    "added field {field_name}=\"{default_val}\"{}",
                    field_def
                        .default
                        .as_ref()
                        .map(|d| format!(" (schema default: {d})"))
                        .unwrap_or_default()
                ),
                applied: true,
            })
        }
        None => Some(FixAction {
            code: "F010".into(),
            description: format!(
                "field \"{field_name}\" has no default — manual fix needed"
            ),
            applied: false,
        }),
    }
}

/// Fix F021: invalid enum value. Replace with closest valid value.
fn fix_invalid_enum(
    doc: &mut Document,
    diag: &validation::Diagnostic,
    type_def: &TypeDef,
) -> Option<FixAction> {
    // Extract field name and invalid value from message:
    // `field "NAME" has invalid value "VALUE"`
    let field_name = extract_quoted(&diag.message)?;
    let invalid_value = extract_nth_quoted(&diag.message, 1)?;

    let field_def = type_def.fields.iter().find(|f| f.name == field_name)?;

    let allowed = match &field_def.field_type {
        FieldType::Enum(vals) => vals,
        _ => return None,
    };

    let candidates: Vec<&str> = allowed.iter().map(|s| s.as_str()).collect();
    // Allow up to half the string length as max edit distance (reasonable threshold)
    let max_dist = (invalid_value.len() / 2).max(2);

    match template::closest_match(&invalid_value, &candidates, max_dist) {
        Some(closest) => {
            doc.set_field_from_str(&field_name, closest);
            Some(FixAction {
                code: "F021".into(),
                description: format!(
                    "field \"{field_name}\": \"{invalid_value}\" → \"{closest}\""
                ),
                applied: true,
            })
        }
        None => Some(FixAction {
            code: "F021".into(),
            description: format!(
                "field \"{field_name}\": no close match for \"{invalid_value}\" in [{}]",
                candidates.join(", ")
            ),
            applied: false,
        }),
    }
}

/// Fix S010: missing required section. Append section heading to document body.
fn fix_missing_section(doc: &mut Document, diag: &validation::Diagnostic) -> Option<FixAction> {
    // Extract section name from message: `missing required section "NAME"`
    let section_name = extract_quoted(&diag.message)?;

    // Handle nested sections like "Consequences > Positive"
    let leaf_name = section_name
        .rsplit(" > ")
        .next()
        .unwrap_or(&section_name);

    // Determine heading level: if nested, use ## etc.
    let depth = section_name.matches(" > ").count() + 1;
    let hashes: String = "#".repeat(depth);

    // Append to body
    let suffix = format!("\n{hashes} {leaf_name}\n\n");
    doc.body.push_str(&suffix);
    // Rebuild raw from frontmatter + body
    doc.raw = rebuild_raw(doc);

    Some(FixAction {
        code: "S010".into(),
        description: format!("added section \"{section_name}\""),
        applied: true,
    })
}

/// Rebuild raw document from frontmatter + body.
fn rebuild_raw(doc: &Document) -> String {
    let mut raw = String::new();
    if let Some(ref fm) = doc.frontmatter {
        raw.push_str("---\n");
        raw.push_str(&fm.to_yaml_string());
        raw.push_str("---\n");
    }
    raw.push_str(&doc.body);
    raw
}

/// Extract the first double-quoted substring from a message.
fn extract_quoted(msg: &str) -> Option<String> {
    extract_nth_quoted(msg, 0)
}

/// Extract the nth double-quoted substring from a message.
fn extract_nth_quoted(msg: &str, n: usize) -> Option<String> {
    let mut count = 0;
    let mut chars = msg.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '"' {
            let mut val = String::new();
            for c in chars.by_ref() {
                if c == '"' {
                    break;
                }
                val.push(c);
            }
            if count == n {
                return Some(val);
            }
            count += 1;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_quoted() {
        assert_eq!(
            extract_quoted(r#"missing required field "title""#),
            Some("title".to_string())
        );
        assert_eq!(
            extract_quoted(r#"missing required section "Decision""#),
            Some("Decision".to_string())
        );
    }

    #[test]
    fn test_extract_nth_quoted() {
        let msg = r#"field "status" has invalid value "aceppted""#;
        assert_eq!(extract_nth_quoted(msg, 0), Some("status".to_string()));
        assert_eq!(extract_nth_quoted(msg, 1), Some("aceppted".to_string()));
    }
}
