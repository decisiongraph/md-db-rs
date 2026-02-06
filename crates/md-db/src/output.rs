use std::io::IsTerminal;

use serde_json::Value;

use crate::frontmatter::{yaml_to_json, yaml_value_to_string};
use crate::table::Table;

/// Re-export for backward compatibility with external callers.
pub use crate::frontmatter::yaml_value_to_string as yaml_value_display;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Text,
    Markdown,
    Json,
    /// One-liner per diagnostic: `code:severity:location:message`
    Compact,
}

impl OutputFormat {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "text" => Some(Self::Text),
            "markdown" | "md" => Some(Self::Markdown),
            "json" => Some(Self::Json),
            "compact" => Some(Self::Compact),
            "auto" => Some(Self::auto()),
            _ => None,
        }
    }

    /// Auto-detect: JSON when stdout is not a TTY, text otherwise.
    pub fn auto() -> Self {
        if std::io::stdout().is_terminal() {
            Self::Text
        } else {
            Self::Json
        }
    }
}

/// Format a frontmatter field value for output.
pub fn format_field_value(val: &serde_yaml::Value, format: OutputFormat) -> String {
    match format {
        OutputFormat::Json => {
            let json = yaml_to_json(val);
            serde_json::to_string(&json).unwrap_or_default()
        }
        _ => yaml_value_to_string(val),
    }
}

/// Format a section for output.
pub fn format_section(content: &str, format: OutputFormat) -> String {
    match format {
        OutputFormat::Markdown => content.to_string(),
        OutputFormat::Json => {
            serde_json::to_string(&Value::String(content.to_string())).unwrap_or_default()
        }
        _ => strip_markdown(content),
    }
}

/// Format a table for output.
pub fn format_table(table: &Table, format: OutputFormat) -> String {
    match format {
        OutputFormat::Json => serde_json::to_string_pretty(&table.to_json()).unwrap_or_default(),
        _ => table.to_text(),
    }
}

/// Format a list of file entries for output.
pub fn format_list(
    entries: &[ListEntry],
    format: OutputFormat,
    fields: &Option<Vec<String>>,
) -> String {
    match format {
        OutputFormat::Json => {
            let arr: Vec<Value> = entries
                .iter()
                .map(|e| {
                    let mut obj = serde_json::Map::new();
                    obj.insert(
                        "path".to_string(),
                        Value::String(e.path.clone()),
                    );
                    if let Some(ref fm) = e.frontmatter_json {
                        match fields {
                            Some(field_list) => {
                                for f in field_list {
                                    if let Some(v) = fm.get(f) {
                                        obj.insert(f.clone(), v.clone());
                                    }
                                }
                            }
                            None => {
                                if let Value::Object(map) = fm {
                                    for (k, v) in map {
                                        obj.insert(k.clone(), v.clone());
                                    }
                                }
                            }
                        }
                    }
                    Value::Object(obj)
                })
                .collect();
            serde_json::to_string_pretty(&arr).unwrap_or_default()
        }
        _ => entries
            .iter()
            .map(|e| e.path.clone())
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

pub struct ListEntry {
    pub path: String,
    pub frontmatter_json: Option<Value>,
}

fn strip_markdown(md: &str) -> String {
    use comrak::{Arena, Options};
    let arena = Arena::new();
    let opts = Options::default();
    let root = comrak::parse_document(&arena, md, &opts);
    crate::ast_util::collect_text_blocks(root)
}
