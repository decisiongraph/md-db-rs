use serde_json::Value;

use crate::table::Table;

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

use std::io::IsTerminal;

/// Format a frontmatter field value for output.
pub fn format_field_value(val: &serde_yaml::Value, format: OutputFormat) -> String {
    match format {
        OutputFormat::Json => {
            let json = yaml_value_to_json(val);
            serde_json::to_string(&json).unwrap_or_default()
        }
        _ => yaml_value_display(val),
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

pub fn yaml_value_display(v: &serde_yaml::Value) -> String {
    match v {
        serde_yaml::Value::Null => "null".to_string(),
        serde_yaml::Value::Bool(b) => b.to_string(),
        serde_yaml::Value::Number(n) => n.to_string(),
        serde_yaml::Value::String(s) => s.clone(),
        serde_yaml::Value::Sequence(seq) => {
            let items: Vec<String> = seq.iter().map(yaml_value_display).collect();
            format!("[{}]", items.join(", "))
        }
        serde_yaml::Value::Mapping(_) => serde_yaml::to_string(v).unwrap_or_default(),
        serde_yaml::Value::Tagged(t) => yaml_value_display(&t.value),
    }
}

fn yaml_value_to_json(v: &serde_yaml::Value) -> Value {
    match v {
        serde_yaml::Value::Null => Value::Null,
        serde_yaml::Value::Bool(b) => Value::Bool(*b),
        serde_yaml::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Number(i.into())
            } else if let Some(f) = n.as_f64() {
                serde_json::Number::from_f64(f)
                    .map(Value::Number)
                    .unwrap_or(Value::Null)
            } else {
                Value::Null
            }
        }
        serde_yaml::Value::String(s) => Value::String(s.clone()),
        serde_yaml::Value::Sequence(seq) => {
            Value::Array(seq.iter().map(yaml_value_to_json).collect())
        }
        serde_yaml::Value::Mapping(map) => {
            let obj: serde_json::Map<String, Value> = map
                .iter()
                .filter_map(|(k, v)| {
                    let key = match k {
                        serde_yaml::Value::String(s) => s.clone(),
                        other => yaml_value_display(other),
                    };
                    Some((key, yaml_value_to_json(v)))
                })
                .collect();
            Value::Object(obj)
        }
        serde_yaml::Value::Tagged(t) => yaml_value_to_json(&t.value),
    }
}
