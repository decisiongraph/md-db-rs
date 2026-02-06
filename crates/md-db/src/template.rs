use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_yaml::Value;

use crate::frontmatter::Frontmatter;
use crate::schema::{FieldDef, FieldType, Schema, SectionDef, TypeDef};

/// Generate a markdown document from a schema type definition.
///
/// `fields` are user-supplied overrides as (key, raw_value_string) pairs.
/// If `fill` is true, date-pattern placeholders are replaced with real dates.
pub fn generate_document(
    type_def: &TypeDef,
    _schema: &Schema,
    fields: &[(String, String)],
) -> String {
    generate_document_opts(type_def, _schema, fields, false)
}

/// Like `generate_document` but with `fill` option to expand all placeholders.
pub fn generate_document_opts(
    type_def: &TypeDef,
    _schema: &Schema,
    fields: &[(String, String)],
    fill: bool,
) -> String {
    let overrides: BTreeMap<&str, &str> = fields.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();

    // Build frontmatter
    let mut data = BTreeMap::new();
    data.insert("type".to_string(), Value::String(type_def.name.clone()));

    for field in &type_def.fields {
        let value = if let Some(&raw) = overrides.get(field.name.as_str()) {
            crate::frontmatter::parse_yaml_value(raw)
        } else {
            default_value(field, fill)
        };
        data.insert(field.name.clone(), value);
    }

    let fm = Frontmatter::from_data(data);
    let mut out = String::new();
    out.push_str("---\n");
    out.push_str(&fm.to_yaml_string());
    out.push_str("---\n");

    // Build body from sections
    for section in &type_def.sections {
        render_section(&mut out, section, 1);
    }

    out
}

fn default_value(field_def: &FieldDef, fill: bool) -> Value {
    // Schema-defined default takes priority
    if let Some(ref default_str) = field_def.default {
        return expand_default(default_str);
    }

    // Check for date-like patterns
    if let Some(ref pat) = field_def.pattern {
        if pat.contains("\\d{4}") && pat.contains("\\d{2}") {
            if fill {
                // --fill: use real dates
                if pat.contains('T') {
                    return Value::String(format_now());
                }
                return Value::String(format_today());
            }
            if pat.contains('T') {
                return Value::String("YYYY-MM-DDT00:00:00Z".to_string());
            }
            return Value::String("YYYY-MM-DD".to_string());
        }
    }

    match &field_def.field_type {
        FieldType::String => Value::String(String::new()),
        FieldType::Number => Value::Number(0.into()),
        FieldType::Bool => Value::Bool(false),
        FieldType::Enum(values) => {
            if let Some(first) = values.first() {
                Value::String(first.clone())
            } else {
                Value::String(String::new())
            }
        }
        FieldType::User => Value::String("@".to_string()),
        FieldType::UserArray => Value::Sequence(vec![]),
        FieldType::Ref => Value::String(String::new()),
        FieldType::RefArray => Value::Sequence(vec![]),
        FieldType::StringArray => Value::Sequence(vec![]),
    }
}

fn expand_default(s: &str) -> Value {
    match s {
        "$TODAY" => Value::String(format_today()),
        "$NOW" => Value::String(format_now()),
        other => crate::frontmatter::parse_yaml_value(other),
    }
}

/// Format current date as YYYY-MM-DD without external crate.
fn format_today() -> String {
    let (year, month, day) = civil_date_from_epoch();
    format!("{year:04}-{month:02}-{day:02}")
}

/// Format current datetime as YYYY-MM-DDTHH:MM:SSZ without external crate.
fn format_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let (year, month, day) = civil_date_from_epoch();
    let day_secs = (secs % 86400) as u32;
    let hour = day_secs / 3600;
    let minute = (day_secs % 3600) / 60;
    let second = day_secs % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Convert current unix timestamp to (year, month, day) in UTC.
fn civil_date_from_epoch() -> (i32, u32, u32) {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = (secs / 86400) as i64;
    // Algorithm from Howard Hinnant's chrono-compatible date library
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d)
}

fn render_section(out: &mut String, section: &SectionDef, depth: u8) {
    // Heading
    out.push('\n');
    for _ in 0..depth {
        out.push('#');
    }
    out.push(' ');
    out.push_str(&section.name);
    out.push('\n');
    out.push('\n');

    // Table scaffold if defined
    if let Some(ref table_def) = section.table {
        let headers: Vec<&str> = table_def.columns.iter().map(|c| c.name.as_str()).collect();
        out.push_str("| ");
        out.push_str(&headers.join(" | "));
        out.push_str(" |\n");

        out.push('|');
        for _ in &table_def.columns {
            out.push_str("---|");
        }
        out.push('\n');
    }

    // Child sections
    for child in &section.children {
        render_section(out, child, depth + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Schema;

    #[test]
    fn test_generate_minimal() {
        let kdl = r#"
type "test" {
    field "title" type="string" required=#true
    field "status" type="enum" required=#true {
        values "draft" "active"
    }
    section "Body" required=#true
}
"#;
        let schema = Schema::from_str(kdl).unwrap();
        let type_def = schema.get_type("test").unwrap();
        let doc = generate_document(type_def, &schema, &[]);

        assert!(doc.contains("type: test"));
        assert!(doc.contains("title:"));
        assert!(doc.contains("status: draft")); // first enum value
        assert!(doc.contains("# Body"));
    }

    #[test]
    fn test_generate_with_overrides() {
        let kdl = r#"
type "test" {
    field "title" type="string" required=#true
    field "status" type="enum" required=#true {
        values "draft" "active"
    }
    section "Body" required=#true
}
"#;
        let schema = Schema::from_str(kdl).unwrap();
        let type_def = schema.get_type("test").unwrap();
        let fields = vec![
            ("title".to_string(), "My Title".to_string()),
            ("status".to_string(), "active".to_string()),
        ];
        let doc = generate_document(type_def, &schema, &fields);

        assert!(doc.contains("title: My Title"));
        assert!(doc.contains("status: active"));
    }

    #[test]
    fn test_generate_with_table_scaffold() {
        let kdl = r#"
type "test" {
    field "title" type="string"
    section "Data" {
        table {
            column "Name" type="string" required=#true
            column "Score" type="number"
        }
    }
}
"#;
        let schema = Schema::from_str(kdl).unwrap();
        let type_def = schema.get_type("test").unwrap();
        let doc = generate_document(type_def, &schema, &[]);

        assert!(doc.contains("| Name | Score |"));
        assert!(doc.contains("|---|---|"));
    }

    #[test]
    fn test_generate_nested_sections() {
        let kdl = r#"
type "test" {
    section "Parent" {
        section "Child"
    }
}
"#;
        let schema = Schema::from_str(kdl).unwrap();
        let type_def = schema.get_type("test").unwrap();
        let doc = generate_document(type_def, &schema, &[]);

        assert!(doc.contains("# Parent"));
        assert!(doc.contains("## Child"));
    }

    #[test]
    fn test_generate_date_pattern_default() {
        let kdl = r#"
type "test" {
    field "date" type="string" pattern="^\\d{4}-\\d{2}-\\d{2}$"
    field "timestamp" type="string" pattern="^\\d{4}-\\d{2}-\\d{2}T"
    section "Body"
}
"#;
        let schema = Schema::from_str(kdl).unwrap();
        let type_def = schema.get_type("test").unwrap();
        let doc = generate_document(type_def, &schema, &[]);

        assert!(doc.contains("YYYY-MM-DD"));
        assert!(doc.contains("YYYY-MM-DDT00:00:00Z"));
    }

    #[test]
    fn test_generate_full_schema() {
        let content = std::fs::read_to_string("../../tests/fixtures/schema.kdl").unwrap();
        let schema = Schema::from_str(&content).unwrap();
        let type_def = schema.get_type("adr").unwrap();
        let fields = vec![
            ("title".to_string(), "Test ADR".to_string()),
            ("status".to_string(), "proposed".to_string()),
            ("author".to_string(), "@onni".to_string()),
            ("date".to_string(), "2025-01-01".to_string()),
        ];
        let doc = generate_document(type_def, &schema, &fields);

        assert!(doc.contains("type: adr"));
        assert!(doc.contains("title: Test ADR"));
        assert!(doc.contains("status: proposed"));
        // serde_yaml may or may not quote strings; just check the values are present
        assert!(doc.contains("@onni"));
        assert!(doc.contains("2025-01-01"));
        assert!(doc.contains("# Decision"));
        assert!(doc.contains("# Consequences"));
        assert!(doc.contains("## Positive"));
        assert!(doc.contains("| Option | Score | Notes |"));
    }

    #[test]
    fn test_schema_default_static() {
        let kdl = r#"
type "test" {
    field "status" type="enum" default="proposed" {
        values "proposed" "accepted"
    }
    section "Body"
}
"#;
        let schema = Schema::from_str(kdl).unwrap();
        let type_def = schema.get_type("test").unwrap();
        let doc = generate_document(type_def, &schema, &[]);
        assert!(doc.contains("status: proposed"));
    }

    #[test]
    fn test_schema_default_today() {
        let kdl = r#"
type "test" {
    field "date" type="string" default="$TODAY"
    section "Body"
}
"#;
        let schema = Schema::from_str(kdl).unwrap();
        let type_def = schema.get_type("test").unwrap();
        let doc = generate_document(type_def, &schema, &[]);
        // Should contain a real date like 2026-02-06, not placeholder
        let re = regex::Regex::new(r"\d{4}-\d{2}-\d{2}").unwrap();
        assert!(re.is_match(&doc), "expected date pattern in: {doc}");
        assert!(!doc.contains("YYYY"), "should not contain placeholder: {doc}");
    }

    #[test]
    fn test_schema_default_now() {
        let kdl = r#"
type "test" {
    field "ts" type="string" default="$NOW"
    section "Body"
}
"#;
        let schema = Schema::from_str(kdl).unwrap();
        let type_def = schema.get_type("test").unwrap();
        let doc = generate_document(type_def, &schema, &[]);
        let re = regex::Regex::new(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z").unwrap();
        assert!(re.is_match(&doc), "expected ISO datetime in: {doc}");
    }

    #[test]
    fn test_override_beats_schema_default() {
        let kdl = r#"
type "test" {
    field "status" type="enum" default="proposed" {
        values "proposed" "accepted"
    }
    section "Body"
}
"#;
        let schema = Schema::from_str(kdl).unwrap();
        let type_def = schema.get_type("test").unwrap();
        let fields = vec![("status".to_string(), "accepted".to_string())];
        let doc = generate_document(type_def, &schema, &fields);
        assert!(doc.contains("status: accepted"));
    }

    #[test]
    fn test_civil_date_sanity() {
        // Just ensure it returns a plausible date
        let (y, m, d) = civil_date_from_epoch();
        assert!(y >= 2024 && y <= 2100);
        assert!((1..=12).contains(&m));
        assert!((1..=31).contains(&d));
    }
}
