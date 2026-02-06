use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

use regex::Regex;

use crate::document::Document;
use comrak::{Arena, Options};
use comrak::nodes::NodeValue;

use crate::schema::{ContentDef, DiagramDef, FieldDef, FieldType, ListDef, Schema, SectionDef, TableDef, TypeDef};
use crate::users::UserConfig;

/// Severity of a validation diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Error,
    Warning,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Severity::Error => write!(f, "error"),
            Severity::Warning => write!(f, "warning"),
        }
    }
}

/// A single validation diagnostic.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: String,
    pub message: String,
    pub location: String,
    pub hint: Option<String>,
}

impl Diagnostic {
    /// One-liner format: `code:severity:location:message`
    pub fn to_compact(&self) -> String {
        format!("{}:{}:{}:{}", self.code, self.severity, self.location, self.message)
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "  {}[{}]: {}", self.severity, self.code, self.message)?;
        write!(f, "\n    --> {}", self.location)?;
        if let Some(ref hint) = self.hint {
            write!(f, "\n    = hint: {hint}")?;
        }
        Ok(())
    }
}

/// Result of validating one or more documents.
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub file_results: Vec<FileResult>,
}

#[derive(Debug, Clone)]
pub struct FileResult {
    pub path: String,
    pub diagnostics: Vec<Diagnostic>,
}

impl FileResult {
    pub fn errors(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count()
    }

    pub fn warnings(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .count()
    }
}

impl ValidationResult {
    pub fn total_errors(&self) -> usize {
        self.file_results.iter().map(|f| f.errors()).sum()
    }

    pub fn total_warnings(&self) -> usize {
        self.file_results.iter().map(|f| f.warnings()).sum()
    }

    pub fn is_ok(&self) -> bool {
        self.total_errors() == 0
    }

    /// Compact format: one line per diagnostic `path:code:severity:location:message`
    pub fn to_compact_report(&self) -> String {
        let mut out = String::new();
        for fr in &self.file_results {
            for d in &fr.diagnostics {
                out.push_str(&fr.path);
                out.push(':');
                out.push_str(&d.to_compact());
                out.push('\n');
            }
        }
        out
    }

    /// Format as human-readable report.
    pub fn to_report(&self) -> String {
        let mut out = String::new();

        for fr in &self.file_results {
            if fr.diagnostics.is_empty() {
                continue;
            }
            out.push_str(&fr.path);
            out.push_str(":\n");
            for d in &fr.diagnostics {
                out.push_str(&format!("{d}\n"));
            }
            out.push('\n');
        }

        let errors = self.total_errors();
        let warnings = self.total_warnings();
        out.push_str(&format!(
            "result: {errors} error(s), {warnings} warning(s)\n"
        ));
        out
    }
}

/// Validate a single document against its type definition in the schema.
pub fn validate_document(
    doc: &Document,
    schema: &Schema,
    known_files: &HashSet<PathBuf>,
    known_ids: &HashSet<String>,
    user_config: Option<&UserConfig>,
) -> FileResult {
    let path = doc
        .path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "<string>".to_string());

    let mut diagnostics = Vec::new();

    // Must have frontmatter
    let fm = match &doc.frontmatter {
        Some(fm) => fm,
        None => {
            diagnostics.push(Diagnostic {
                severity: Severity::Error,
                code: "F000".into(),
                message: "document has no frontmatter".into(),
                location: "frontmatter".into(),
                hint: Some("add YAML frontmatter between --- delimiters".into()),
            });
            return FileResult { path, diagnostics };
        }
    };

    // Must have `type` field
    let type_name = match fm.get_display("type") {
        Some(t) => t,
        None => {
            diagnostics.push(Diagnostic {
                severity: Severity::Error,
                code: "F001".into(),
                message: "missing required field \"type\"".into(),
                location: "frontmatter".into(),
                hint: Some("add 'type: <typename>' to frontmatter".into()),
            });
            return FileResult { path, diagnostics };
        }
    };

    // Look up type definition
    let type_def = match schema.get_type(&type_name) {
        Some(t) => t,
        None => {
            let known: Vec<&str> = schema.types.iter().map(|t| t.name.as_str()).collect();
            diagnostics.push(Diagnostic {
                severity: Severity::Error,
                code: "F002".into(),
                message: format!("unknown document type \"{type_name}\""),
                location: "frontmatter.type".into(),
                hint: Some(format!("known types: {}", known.join(", "))),
            });
            return FileResult { path, diagnostics };
        }
    };

    // Validate fields
    validate_fields(fm, type_def, schema, known_files, known_ids, &doc.path, user_config, &mut diagnostics);

    // Validate conditional rules (if/then constraints)
    validate_rules(fm, type_def, &mut diagnostics);

    // Validate relation fields (defined at schema level, not per-type)
    validate_relation_fields(fm, schema, known_files, known_ids, &doc.path, &mut diagnostics);

    // Validate sections
    validate_sections(doc, &type_def.sections, &[], user_config, &mut diagnostics);

    FileResult { path, diagnostics }
}

fn validate_fields(
    fm: &crate::frontmatter::Frontmatter,
    type_def: &TypeDef,
    schema: &Schema,
    known_files: &HashSet<PathBuf>,
    known_ids: &HashSet<String>,
    doc_path: &Option<PathBuf>,
    user_config: Option<&UserConfig>,
    diags: &mut Vec<Diagnostic>,
) {
    for field_def in &type_def.fields {
        let val = fm.get(&field_def.name);

        // Required check
        if field_def.required && val.is_none() {
            let mut hint = format!(
                "add '{}: <{}>' to frontmatter",
                field_def.name, field_def.field_type
            );
            if let Some(ref desc) = field_def.description {
                hint.push_str(&format!(" — {desc}"));
            }
            diags.push(Diagnostic {
                severity: Severity::Error,
                code: "F010".into(),
                message: format!("missing required field \"{}\"", field_def.name),
                location: "frontmatter".into(),
                hint: Some(hint),
            });
            continue;
        }

        let val = match val {
            Some(v) => v,
            None => continue,
        };

        // Type check
        validate_field_value(&field_def.name, val, field_def, schema, known_files, known_ids, doc_path, user_config, diags);
    }
}

/// Validate conditional rules: when a field matches a value, other fields become required.
fn validate_rules(
    fm: &crate::frontmatter::Frontmatter,
    type_def: &TypeDef,
    diags: &mut Vec<Diagnostic>,
) {
    for rule in &type_def.rules {
        if let Some(val) = fm.get(&rule.when_field) {
            let val_str = match val.as_str() {
                Some(s) => s.to_string(),
                None => continue,
            };
            if val_str == rule.when_equals {
                for required_field in &rule.then_required {
                    if fm.get(required_field).is_none() {
                        diags.push(Diagnostic {
                            severity: Severity::Error,
                            code: "F040".into(),
                            message: format!(
                                "field \"{}\" required when {}={}",
                                required_field, rule.when_field, rule.when_equals
                            ),
                            location: format!("frontmatter.{}", required_field),
                            hint: Some(format!(
                                "add '{}' to frontmatter (required by rule \"{}\")",
                                required_field, rule.name
                            )),
                        });
                    }
                }
            }
        }
    }
}

/// Validate relation fields. Relations are defined at schema level and apply to all types.
/// Any frontmatter field matching a relation name/inverse is validated as a ref.
fn validate_relation_fields(
    fm: &crate::frontmatter::Frontmatter,
    schema: &Schema,
    known_files: &HashSet<PathBuf>,
    known_ids: &HashSet<String>,
    doc_path: &Option<PathBuf>,
    diags: &mut Vec<Diagnostic>,
) {
    for key in fm.keys() {
        if let Some((rel_def, _is_inverse)) = schema.find_relation(key) {
            let val = match fm.get(key) {
                Some(v) => v,
                None => continue,
            };

            match rel_def.cardinality {
                crate::schema::Cardinality::One => {
                    // Single ref
                    if let Some(s) = val.as_str() {
                        validate_ref(key, s, schema, known_files, known_ids, doc_path, diags);
                    } else {
                        diags.push(type_mismatch(key, "ref (string)", val));
                    }
                }
                crate::schema::Cardinality::Many => {
                    // Array of refs
                    match val.as_sequence() {
                        Some(seq) => {
                            for (i, item) in seq.iter().enumerate() {
                                if let Some(s) = item.as_str() {
                                    validate_ref(
                                        &format!("{key}[{i}]"),
                                        s,
                                        schema,
                                        known_files,
                                        known_ids,
                                        doc_path,
                                        diags,
                                    );
                                } else {
                                    diags.push(Diagnostic {
                                        severity: Severity::Error,
                                        code: "F020".into(),
                                        message: format!(
                                            "relation \"{key}[{i}]\" expected ref (string), got {}",
                                            yaml_type_name(item)
                                        ),
                                        location: format!("frontmatter.{key}[{i}]"),
                                        hint: None,
                                    });
                                }
                            }
                        }
                        None => {
                            // Allow single string for cardinality=many (auto-wrap)
                            if let Some(s) = val.as_str() {
                                validate_ref(key, s, schema, known_files, known_ids, doc_path, diags);
                            } else {
                                diags.push(type_mismatch(key, "ref[]", val));
                            }
                        }
                    }
                }
            }
        }
    }
}

fn validate_field_value(
    field_name: &str,
    val: &serde_yaml::Value,
    field_def: &FieldDef,
    schema: &Schema,
    known_files: &HashSet<PathBuf>,
    known_ids: &HashSet<String>,
    doc_path: &Option<PathBuf>,
    user_config: Option<&UserConfig>,
    diags: &mut Vec<Diagnostic>,
) {
    match &field_def.field_type {
        FieldType::String => {
            if !val.is_string() {
                diags.push(type_mismatch(field_name, "string", val));
            } else if let Some(ref pattern) = field_def.pattern {
                check_pattern(field_name, val.as_str().unwrap(), pattern, diags);
            }
        }
        FieldType::Number => {
            if !val.is_number() {
                diags.push(type_mismatch(field_name, "number", val));
            }
        }
        FieldType::Bool => {
            if !val.is_bool() {
                diags.push(type_mismatch(field_name, "bool", val));
            }
        }
        FieldType::Enum(allowed) => {
            match val.as_str() {
                Some(s) => {
                    if !allowed.contains(&s.to_string()) {
                        diags.push(Diagnostic {
                            severity: Severity::Error,
                            code: "F021".into(),
                            message: format!(
                                "field \"{field_name}\" has invalid value \"{s}\""
                            ),
                            location: format!("frontmatter.{field_name}"),
                            hint: Some(format!(
                                "allowed values: {}",
                                allowed.join(", ")
                            )),
                        });
                    }
                }
                None => {
                    diags.push(type_mismatch(field_name, "enum (string)", val));
                }
            }
        }
        FieldType::Ref => {
            if let Some(s) = val.as_str() {
                validate_ref(field_name, s, schema, known_files, known_ids, doc_path, diags);
            } else {
                diags.push(type_mismatch(field_name, "ref (string)", val));
            }
        }
        FieldType::StringArray => {
            match val.as_sequence() {
                Some(seq) => {
                    for (i, item) in seq.iter().enumerate() {
                        if !item.is_string() {
                            diags.push(Diagnostic {
                                severity: Severity::Error,
                                code: "F020".into(),
                                message: format!(
                                    "field \"{field_name}[{i}]\" expected string, got {}", yaml_type_name(item)
                                ),
                                location: format!("frontmatter.{field_name}[{i}]"),
                                hint: None,
                            });
                        }
                    }
                    if let Some(ref pattern) = field_def.pattern {
                        for (i, item) in seq.iter().enumerate() {
                            if let Some(s) = item.as_str() {
                                check_pattern(&format!("{field_name}[{i}]"), s, pattern, diags);
                            }
                        }
                    }
                }
                None => {
                    diags.push(type_mismatch(field_name, "string[]", val));
                }
            }
        }
        FieldType::RefArray => {
            match val.as_sequence() {
                Some(seq) => {
                    for (i, item) in seq.iter().enumerate() {
                        if let Some(s) = item.as_str() {
                            validate_ref(
                                &format!("{field_name}[{i}]"),
                                s,
                                schema,
                                known_files,
                                known_ids,
                                doc_path,
                                diags,
                            );
                        } else {
                            diags.push(Diagnostic {
                                severity: Severity::Error,
                                code: "F020".into(),
                                message: format!(
                                    "field \"{field_name}[{i}]\" expected ref (string), got {}",
                                    yaml_type_name(item)
                                ),
                                location: format!("frontmatter.{field_name}[{i}]"),
                                hint: None,
                            });
                        }
                    }
                }
                None => {
                    diags.push(type_mismatch(field_name, "ref[]", val));
                }
            }
        }
        FieldType::User => {
            if let Some(s) = val.as_str() {
                validate_user_ref(field_name, s, user_config, diags);
            } else {
                diags.push(type_mismatch(field_name, "user (@handle)", val));
            }
        }
        FieldType::UserArray => {
            match val.as_sequence() {
                Some(seq) => {
                    for (i, item) in seq.iter().enumerate() {
                        if let Some(s) = item.as_str() {
                            validate_user_ref(&format!("{field_name}[{i}]"), s, user_config, diags);
                        } else {
                            diags.push(Diagnostic {
                                severity: Severity::Error,
                                code: "F020".into(),
                                message: format!(
                                    "field \"{field_name}[{i}]\" expected user (@handle), got {}",
                                    yaml_type_name(item)
                                ),
                                location: format!("frontmatter.{field_name}[{i}]"),
                                hint: None,
                            });
                        }
                    }
                }
                None => {
                    diags.push(type_mismatch(field_name, "user[]", val));
                }
            }
        }
    }
}

fn validate_ref(
    field_name: &str,
    value: &str,
    schema: &Schema,
    known_files: &HashSet<PathBuf>,
    known_ids: &HashSet<String>,
    doc_path: &Option<PathBuf>,
    diags: &mut Vec<Diagnostic>,
) {
    // Check if it matches any ref-format pattern
    let matches_format = schema.ref_formats.iter().any(|rf| {
        Regex::new(&rf.pattern)
            .map(|re| re.is_match(value))
            .unwrap_or(false)
    });

    if !matches_format && !schema.ref_formats.is_empty() {
        let patterns: Vec<&str> = schema.ref_formats.iter().map(|rf| rf.pattern.as_str()).collect();
        diags.push(Diagnostic {
            severity: Severity::Warning,
            code: "R001".into(),
            message: format!("ref \"{value}\" in \"{field_name}\" doesn't match any ref-format"),
            location: format!("frontmatter.{field_name}"),
            hint: Some(format!("expected patterns: {}", patterns.join(", "))),
        });
        return;
    }

    // If it looks like a relative path, check file existence
    if value.ends_with(".md") {
        if let Some(ref base) = doc_path {
            if let Some(dir) = base.parent() {
                let target = dir.join(value);
                if !known_files.contains(&target) {
                    // Try canonical
                    let canonical = target
                        .canonicalize()
                        .ok()
                        .map(|p| known_files.contains(&p))
                        .unwrap_or(false);
                    if !canonical {
                        diags.push(Diagnostic {
                            severity: Severity::Error,
                            code: "R010".into(),
                            message: format!(
                                "broken file reference \"{value}\" in \"{field_name}\""
                            ),
                            location: format!("frontmatter.{field_name}"),
                            hint: Some(format!("resolved to: {}", target.display())),
                        });
                    }
                }
            }
        }
    } else {
        // String ID — check against known IDs
        if !known_ids.contains(value) && !known_ids.is_empty() {
            diags.push(Diagnostic {
                severity: Severity::Warning,
                code: "R011".into(),
                message: format!(
                    "unresolved reference \"{value}\" in \"{field_name}\""
                ),
                location: format!("frontmatter.{field_name}"),
                hint: Some("no document with matching ID found in scope".into()),
            });
        }
    }
}

/// Validate a user/team reference (`@handle` or `@team/name`).
fn validate_user_ref(
    field_name: &str,
    value: &str,
    user_config: Option<&UserConfig>,
    diags: &mut Vec<Diagnostic>,
) {
    // Must start with @
    if !value.starts_with('@') {
        diags.push(Diagnostic {
            severity: Severity::Error,
            code: "U010".into(),
            message: format!(
                "field \"{field_name}\" value \"{value}\" is not a valid user reference"
            ),
            location: format!("frontmatter.{field_name}"),
            hint: Some("user references must start with @ (e.g. @onni, @team/platform)".into()),
        });
        return;
    }

    // If user config is provided, validate the reference resolves
    if let Some(config) = user_config {
        if !config.is_valid_ref(value) {
            let mut all_refs = config.all_user_handles();
            all_refs.extend(config.all_team_names());
            diags.push(Diagnostic {
                severity: Severity::Error,
                code: "U011".into(),
                message: format!(
                    "field \"{field_name}\" references unknown user/team \"{value}\""
                ),
                location: format!("frontmatter.{field_name}"),
                hint: if all_refs.is_empty() {
                    None
                } else {
                    Some(format!("known: {}", all_refs.join(", ")))
                },
            });
        }
    }
}

fn validate_sections(
    doc: &Document,
    section_defs: &[SectionDef],
    parent_path: &[&str],
    user_config: Option<&UserConfig>,
    diags: &mut Vec<Diagnostic>,
) {
    for sec_def in section_defs {
        let section_result = if parent_path.is_empty() {
            doc.get_section(&sec_def.name)
        } else {
            let mut full_path: Vec<&str> = parent_path.to_vec();
            full_path.push(&sec_def.name);
            doc.get_section_by_path(&full_path)
        };

        match section_result {
            Ok(section) => {
                // Validate table if defined
                if let Some(ref table_def) = sec_def.table {
                    let tables = section.tables();
                    if tables.is_empty() && table_def.required {
                        diags.push(Diagnostic {
                            severity: Severity::Error,
                            code: "S020".into(),
                            message: format!(
                                "section \"{}\" requires a table but none found",
                                sec_def.name
                            ),
                            location: format!("section \"{}\"", sec_def.name),
                            hint: Some("add a markdown table to this section".into()),
                        });
                    } else if let Some(table) = tables.first() {
                        validate_table_columns(table, table_def, &sec_def.name, user_config, diags);
                    }
                }

                // Content constraint
                if let Some(ref content_def) = sec_def.content {
                    validate_content_constraint(&section, content_def, &sec_def.name, diags);
                }

                // List constraint
                if let Some(ref list_def) = sec_def.list {
                    validate_list_constraint(&section, list_def, &sec_def.name, diags);
                }

                // Diagram constraint
                if let Some(ref diagram_def) = sec_def.diagram {
                    validate_diagram_constraint(&section, diagram_def, &sec_def.name, diags);
                }

                // Recurse into child sections
                if !sec_def.children.is_empty() {
                    let mut path: Vec<&str> = parent_path.to_vec();
                    path.push(&sec_def.name);
                    validate_sections(doc, &sec_def.children, &path, user_config, diags);
                }
            }
            Err(_) => {
                if sec_def.required {
                    let full_name = if parent_path.is_empty() {
                        sec_def.name.clone()
                    } else {
                        format!("{} > {}", parent_path.join(" > "), sec_def.name)
                    };
                    let mut hint = format!(
                        "add heading: \"# {}\" or \"## {}\"",
                        sec_def.name, sec_def.name
                    );
                    if let Some(ref desc) = sec_def.description {
                        hint.push_str(&format!(" — {desc}"));
                    }
                    diags.push(Diagnostic {
                        severity: Severity::Error,
                        code: "S010".into(),
                        message: format!("missing required section \"{full_name}\""),
                        location: "document body".into(),
                        hint: Some(hint),
                    });
                }
            }
        }
    }
}

/// Validate table columns: required columns present + user type columns.
fn validate_table_columns(
    table: &crate::table::Table,
    table_def: &TableDef,
    section_name: &str,
    user_config: Option<&UserConfig>,
    diags: &mut Vec<Diagnostic>,
) {
    for col_def in &table_def.columns {
        if col_def.required && !table.headers().iter().any(|h| h == &col_def.name) {
            diags.push(Diagnostic {
                severity: Severity::Error,
                code: "S021".into(),
                message: format!(
                    "table in \"{}\" missing required column \"{}\"",
                    section_name, col_def.name
                ),
                location: format!("section \"{section_name}\" > table"),
                hint: None,
            });
            continue;
        }

        // Validate user-typed column cells
        if col_def.col_type == FieldType::User {
            if let Some(col_values) = table.get_column(&col_def.name) {
                for (row_idx, cell) in col_values.iter().enumerate() {
                    let cell = cell.trim();
                    if cell.is_empty() {
                        if col_def.required {
                            diags.push(Diagnostic {
                                severity: Severity::Error,
                                code: "S022".into(),
                                message: format!(
                                    "table in \"{section_name}\" column \"{}\" row {row_idx} is empty but required",
                                    col_def.name
                                ),
                                location: format!("section \"{section_name}\" > table > {}[{row_idx}]", col_def.name),
                                hint: None,
                            });
                        }
                        continue;
                    }
                    validate_user_ref(
                        &format!("table:{section_name}.{}.row{row_idx}", col_def.name),
                        cell,
                        user_config,
                        diags,
                    );
                }
            }
        }
    }
}

/// Known diagram languages for fenced code blocks.
const DIAGRAM_LANGUAGES: &[&str] = &["mermaid", "d2", "plantuml", "graphviz", "dot"];

fn validate_content_constraint(
    section: &crate::section::Section,
    content_def: &ContentDef,
    section_name: &str,
    diags: &mut Vec<Diagnostic>,
) {
    let arena = Arena::new();
    let opts = Options::default();
    let root = comrak::parse_document(&arena, &section.content, &opts);

    let paragraph_count = root
        .descendants()
        .filter(|n| matches!(n.data.borrow().value, NodeValue::Paragraph))
        .count();

    if let Some(min) = content_def.min_paragraphs {
        if paragraph_count < min {
            diags.push(Diagnostic {
                severity: Severity::Error,
                code: "S030".into(),
                message: format!(
                    "section \"{section_name}\" requires at least {min} paragraph(s), found {paragraph_count}"
                ),
                location: format!("section \"{section_name}\""),
                hint: Some("add prose content to this section".into()),
            });
        }
    }
}

fn validate_list_constraint(
    section: &crate::section::Section,
    list_def: &ListDef,
    section_name: &str,
    diags: &mut Vec<Diagnostic>,
) {
    let arena = Arena::new();
    let opts = Options::default();
    let root = comrak::parse_document(&arena, &section.content, &opts);

    let lists: Vec<_> = root
        .descendants()
        .filter(|n| matches!(n.data.borrow().value, NodeValue::List(_)))
        .collect();

    if lists.is_empty() && list_def.required {
        diags.push(Diagnostic {
            severity: Severity::Error,
            code: "S031".into(),
            message: format!("section \"{section_name}\" requires a list but none found"),
            location: format!("section \"{section_name}\""),
            hint: Some("add a markdown list (- item) to this section".into()),
        });
        return;
    }

    if let Some(min_items) = list_def.min_items {
        // Count items across all lists in the section
        let total_items: usize = lists
            .iter()
            .map(|list_node| {
                list_node
                    .children()
                    .filter(|n| matches!(n.data.borrow().value, NodeValue::Item(_)))
                    .count()
            })
            .sum();

        if total_items < min_items {
            diags.push(Diagnostic {
                severity: Severity::Error,
                code: "S031".into(),
                message: format!(
                    "section \"{section_name}\" requires at least {min_items} list item(s), found {total_items}"
                ),
                location: format!("section \"{section_name}\""),
                hint: Some(format!("add at least {min_items} list items")),
            });
        }
    }
}

fn validate_diagram_constraint(
    section: &crate::section::Section,
    diagram_def: &DiagramDef,
    section_name: &str,
    diags: &mut Vec<Diagnostic>,
) {
    let arena = Arena::new();
    let opts = Options::default();
    let root = comrak::parse_document(&arena, &section.content, &opts);

    let code_blocks: Vec<String> = root
        .descendants()
        .filter_map(|n| {
            if let NodeValue::CodeBlock(ref cb) = n.data.borrow().value {
                Some(cb.info.trim().to_lowercase())
            } else {
                None
            }
        })
        .collect();

    let has_diagram = if let Some(ref expected_type) = diagram_def.diagram_type {
        let expected = expected_type.to_lowercase();
        code_blocks.iter().any(|info| info == &expected)
    } else {
        code_blocks
            .iter()
            .any(|info| DIAGRAM_LANGUAGES.iter().any(|lang| info == lang))
    };

    if !has_diagram && diagram_def.required {
        let hint = if let Some(ref dt) = diagram_def.diagram_type {
            format!("add a ```{dt} code block to this section")
        } else {
            format!(
                "add a fenced code block with a diagram language ({})",
                DIAGRAM_LANGUAGES.join(", ")
            )
        };
        diags.push(Diagnostic {
            severity: Severity::Error,
            code: "S032".into(),
            message: format!(
                "section \"{section_name}\" requires a diagram but none found"
            ),
            location: format!("section \"{section_name}\""),
            hint: Some(hint),
        });
    }
}

fn check_pattern(field_name: &str, value: &str, pattern: &str, diags: &mut Vec<Diagnostic>) {
    match Regex::new(pattern) {
        Ok(re) => {
            if !re.is_match(value) {
                diags.push(Diagnostic {
                    severity: Severity::Error,
                    code: "F030".into(),
                    message: format!(
                        "field \"{field_name}\" value \"{value}\" doesn't match pattern"
                    ),
                    location: format!("frontmatter.{field_name}"),
                    hint: Some(format!("expected pattern: {pattern}")),
                });
            }
        }
        Err(e) => {
            diags.push(Diagnostic {
                severity: Severity::Warning,
                code: "S000".into(),
                message: format!("invalid regex pattern in schema for \"{field_name}\": {e}"),
                location: "schema".into(),
                hint: None,
            });
        }
    }
}

fn type_mismatch(field_name: &str, expected: &str, got: &serde_yaml::Value) -> Diagnostic {
    Diagnostic {
        severity: Severity::Error,
        code: "F020".into(),
        message: format!(
            "field \"{field_name}\" expected {expected}, got {}",
            yaml_type_name(got)
        ),
        location: format!("frontmatter.{field_name}"),
        hint: None,
    }
}

/// Validate that no type exceeds its max_count.
fn validate_type_counts(
    files: &[PathBuf],
    schema: &Schema,
    file_results: &mut Vec<FileResult>,
) {
    // Count documents per type
    let mut type_counts: HashMap<String, Vec<String>> = HashMap::new();
    for path in files {
        if let Ok(doc) = Document::from_file(path) {
            if let Some(ref fm) = doc.frontmatter {
                if let Some(type_name) = fm.get_display("type") {
                    type_counts
                        .entry(type_name)
                        .or_default()
                        .push(path.display().to_string());
                }
            }
        }
    }

    for type_def in &schema.types {
        if let Some(max) = type_def.max_count {
            if let Some(paths) = type_counts.get(&type_def.name) {
                if paths.len() > max {
                    // Add diagnostic to the first file that exceeds the limit
                    let diag = Diagnostic {
                        severity: Severity::Error,
                        code: "T010".into(),
                        message: format!(
                            "type \"{}\" has {} document(s) but max_count is {}",
                            type_def.name,
                            paths.len(),
                            max
                        ),
                        location: format!("type \"{}\"", type_def.name),
                        hint: Some(format!(
                            "files: {}",
                            paths.join(", ")
                        )),
                    };
                    // Attach to the first excess file
                    if let Some(excess_path) = paths.get(max) {
                        // Find or create a FileResult for this path
                        if let Some(fr) = file_results.iter_mut().find(|fr| fr.path == *excess_path) {
                            fr.diagnostics.push(diag);
                        } else {
                            file_results.push(FileResult {
                                path: excess_path.clone(),
                                diagnostics: vec![diag],
                            });
                        }
                    }
                }
            }
        }
    }
}

fn yaml_type_name(v: &serde_yaml::Value) -> &'static str {
    match v {
        serde_yaml::Value::Null => "null",
        serde_yaml::Value::Bool(_) => "bool",
        serde_yaml::Value::Number(_) => "number",
        serde_yaml::Value::String(_) => "string",
        serde_yaml::Value::Sequence(_) => "array",
        serde_yaml::Value::Mapping(_) => "mapping",
        serde_yaml::Value::Tagged(_) => "tagged",
    }
}

/// Validate all markdown files in a directory against a schema.
pub fn validate_directory(
    dir: impl AsRef<Path>,
    schema: &Schema,
    pattern: Option<&str>,
    user_config: Option<&UserConfig>,
) -> crate::error::Result<ValidationResult> {
    let files = crate::discovery::discover_files(&dir, pattern, &[])?;

    // Build known file set and known ID set for cross-ref validation
    let known_files: HashSet<PathBuf> = files
        .iter()
        .map(|p| p.canonicalize().unwrap_or_else(|_| p.clone()))
        .collect();

    let mut known_ids: HashSet<String> = HashSet::new();
    // Extract IDs from filenames: adr-001.md -> ADR-001
    // Handles slugged filenames: adr-001-use-postgresql.md -> ADR-001
    for path in &files {
        known_ids.insert(crate::graph::path_to_id(path));
    }

    let mut file_results = Vec::new();
    for path in &files {
        let doc = match Document::from_file(path) {
            Ok(d) => d,
            Err(e) => {
                file_results.push(FileResult {
                    path: path.display().to_string(),
                    diagnostics: vec![Diagnostic {
                        severity: Severity::Error,
                        code: "E000".into(),
                        message: format!("failed to parse: {e}"),
                        location: "file".into(),
                        hint: None,
                    }],
                });
                continue;
            }
        };

        // Skip files without frontmatter type (not managed by schema)
        if doc.frontmatter.is_none() {
            continue;
        }
        if let Some(ref fm) = doc.frontmatter {
            if fm.get("type").is_none() {
                continue;
            }
        }

        file_results.push(validate_document(&doc, schema, &known_files, &known_ids, user_config));
    }

    // Validate max_count per type
    validate_type_counts(&files, schema, &mut file_results);

    Ok(ValidationResult { file_results })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_schema() -> Schema {
        Schema::from_str(
            r#"
type "adr" {
    field "title" type="string" required=#true
    field "status" type="enum" required=#true {
        values "proposed" "accepted" "rejected"
    }
    field "author" type="string" required=#true pattern="^@.+"
    section "Decision" required=#true
    section "Consequences" required=#true {
        section "Positive" required=#true
    }
}
ref-format {
    string-id pattern="^ADR-\\d+$"
}
"#,
        )
        .unwrap()
    }

    #[test]
    fn test_valid_document() {
        let doc = Document::from_str(
            "---\ntype: adr\ntitle: Test\nstatus: accepted\nauthor: \"@onni\"\n---\n\n# Decision\n\nWe decided.\n\n# Consequences\n\n## Positive\n\nGood.\n",
        )
        .unwrap();
        let schema = test_schema();
        let result = validate_document(&doc, &schema, &HashSet::new(), &HashSet::new(), None);
        assert_eq!(result.errors(), 0, "diagnostics: {:?}", result.diagnostics);
    }

    #[test]
    fn test_missing_required_field() {
        let doc =
            Document::from_str("---\ntype: adr\ntitle: Test\nstatus: accepted\n---\n\n# Decision\n\nX\n\n# Consequences\n\n## Positive\n\nY\n")
                .unwrap();
        let schema = test_schema();
        let result = validate_document(&doc, &schema, &HashSet::new(), &HashSet::new(), None);
        assert!(result.errors() > 0);
        assert!(result.diagnostics.iter().any(|d| d.code == "F010" && d.message.contains("author")));
    }

    #[test]
    fn test_invalid_enum_value() {
        let doc = Document::from_str(
            "---\ntype: adr\ntitle: T\nstatus: invalid\nauthor: \"@x\"\n---\n\n# Decision\n\nX\n\n# Consequences\n\n## Positive\n\nY\n",
        )
        .unwrap();
        let schema = test_schema();
        let result = validate_document(&doc, &schema, &HashSet::new(), &HashSet::new(), None);
        assert!(result.diagnostics.iter().any(|d| d.code == "F021"));
    }

    #[test]
    fn test_pattern_mismatch() {
        let doc = Document::from_str(
            "---\ntype: adr\ntitle: T\nstatus: accepted\nauthor: badformat\n---\n\n# Decision\n\nX\n\n# Consequences\n\n## Positive\n\nY\n",
        )
        .unwrap();
        let schema = test_schema();
        let result = validate_document(&doc, &schema, &HashSet::new(), &HashSet::new(), None);
        assert!(result.diagnostics.iter().any(|d| d.code == "F030"));
    }

    #[test]
    fn test_missing_required_section() {
        let doc = Document::from_str(
            "---\ntype: adr\ntitle: T\nstatus: accepted\nauthor: \"@x\"\n---\n\n# Decision\n\nX\n",
        )
        .unwrap();
        let schema = test_schema();
        let result = validate_document(&doc, &schema, &HashSet::new(), &HashSet::new(), None);
        assert!(result.diagnostics.iter().any(|d| d.code == "S010" && d.message.contains("Consequences")));
    }

    #[test]
    fn test_unknown_type() {
        let doc = Document::from_str("---\ntype: unknown\ntitle: T\n---\n\n# Body\n").unwrap();
        let schema = test_schema();
        let result = validate_document(&doc, &schema, &HashSet::new(), &HashSet::new(), None);
        assert!(result.diagnostics.iter().any(|d| d.code == "F002"));
    }

    fn user_schema() -> Schema {
        Schema::from_str(
            r#"
type "doc" {
    field "title" type="string" required=#true
    field "author" type="user" required=#true
    field "reviewers" type="user[]"
    section "Body" required=#true
}
"#,
        )
        .unwrap()
    }

    fn test_user_config() -> UserConfig {
        UserConfig::from_str(
            r##"
users:
  onni:
    name: Onni Hakala
    teams: [platform]
  alice:
    name: Alice Smith
    teams: [platform]
teams:
  platform:
    name: Platform Team
"##,
        )
        .unwrap()
    }

    #[test]
    fn test_valid_user_field() {
        let doc = Document::from_str(
            "---\ntype: doc\ntitle: T\nauthor: \"@onni\"\n---\n\n# Body\n\nContent\n",
        )
        .unwrap();
        let schema = user_schema();
        let uc = test_user_config();
        let result = validate_document(&doc, &schema, &HashSet::new(), &HashSet::new(), Some(&uc));
        assert_eq!(result.errors(), 0, "diagnostics: {:?}", result.diagnostics);
    }

    #[test]
    fn test_invalid_user_no_at() {
        let doc = Document::from_str(
            "---\ntype: doc\ntitle: T\nauthor: onni\n---\n\n# Body\n\nContent\n",
        )
        .unwrap();
        let schema = user_schema();
        let uc = test_user_config();
        let result = validate_document(&doc, &schema, &HashSet::new(), &HashSet::new(), Some(&uc));
        assert!(result.diagnostics.iter().any(|d| d.code == "U010"));
    }

    #[test]
    fn test_unknown_user_ref() {
        let doc = Document::from_str(
            "---\ntype: doc\ntitle: T\nauthor: \"@unknown\"\n---\n\n# Body\n\nContent\n",
        )
        .unwrap();
        let schema = user_schema();
        let uc = test_user_config();
        let result = validate_document(&doc, &schema, &HashSet::new(), &HashSet::new(), Some(&uc));
        assert!(result.diagnostics.iter().any(|d| d.code == "U011"));
    }

    #[test]
    fn test_valid_user_array() {
        let doc = Document::from_str(
            "---\ntype: doc\ntitle: T\nauthor: \"@onni\"\nreviewers:\n  - \"@alice\"\n  - \"@team/platform\"\n---\n\n# Body\n\nContent\n",
        )
        .unwrap();
        let schema = user_schema();
        let uc = test_user_config();
        let result = validate_document(&doc, &schema, &HashSet::new(), &HashSet::new(), Some(&uc));
        assert_eq!(result.errors(), 0, "diagnostics: {:?}", result.diagnostics);
    }

    #[test]
    fn test_user_without_config_only_format_check() {
        // Without UserConfig, only @-prefix format is checked
        let doc = Document::from_str(
            "---\ntype: doc\ntitle: T\nauthor: \"@anyone\"\n---\n\n# Body\n\nContent\n",
        )
        .unwrap();
        let schema = user_schema();
        let result = validate_document(&doc, &schema, &HashSet::new(), &HashSet::new(), None);
        assert_eq!(result.errors(), 0, "diagnostics: {:?}", result.diagnostics);
    }

    // ─── Content constraint tests ────────────────────────────────────────

    fn content_schema() -> Schema {
        Schema::from_str(
            r#"
type "doc" {
    field "title" type="string"
    section "Body" required=#true {
        content min-paragraphs=2
    }
}
"#,
        )
        .unwrap()
    }

    #[test]
    fn test_content_constraint_pass() {
        let doc = Document::from_str(
            "---\ntype: doc\ntitle: T\n---\n\n# Body\n\nFirst paragraph.\n\nSecond paragraph.\n",
        )
        .unwrap();
        let schema = content_schema();
        let result = validate_document(&doc, &schema, &HashSet::new(), &HashSet::new(), None);
        assert_eq!(result.errors(), 0, "diagnostics: {:?}", result.diagnostics);
    }

    #[test]
    fn test_content_constraint_fail() {
        let doc = Document::from_str(
            "---\ntype: doc\ntitle: T\n---\n\n# Body\n\nOnly one paragraph.\n",
        )
        .unwrap();
        let schema = content_schema();
        let result = validate_document(&doc, &schema, &HashSet::new(), &HashSet::new(), None);
        assert!(result.diagnostics.iter().any(|d| d.code == "S030"));
    }

    fn list_schema() -> Schema {
        Schema::from_str(
            r#"
type "doc" {
    field "title" type="string"
    section "Reqs" required=#true {
        list min-items=2
    }
}
"#,
        )
        .unwrap()
    }

    #[test]
    fn test_list_constraint_pass() {
        let doc = Document::from_str(
            "---\ntype: doc\ntitle: T\n---\n\n# Reqs\n\n- Item one\n- Item two\n",
        )
        .unwrap();
        let schema = list_schema();
        let result = validate_document(&doc, &schema, &HashSet::new(), &HashSet::new(), None);
        assert_eq!(result.errors(), 0, "diagnostics: {:?}", result.diagnostics);
    }

    #[test]
    fn test_list_constraint_missing() {
        let doc = Document::from_str(
            "---\ntype: doc\ntitle: T\n---\n\n# Reqs\n\nJust text.\n",
        )
        .unwrap();
        let schema = list_schema();
        let result = validate_document(&doc, &schema, &HashSet::new(), &HashSet::new(), None);
        assert!(result.diagnostics.iter().any(|d| d.code == "S031"));
    }

    #[test]
    fn test_list_constraint_too_few() {
        let doc = Document::from_str(
            "---\ntype: doc\ntitle: T\n---\n\n# Reqs\n\n- Only one\n",
        )
        .unwrap();
        let schema = list_schema();
        let result = validate_document(&doc, &schema, &HashSet::new(), &HashSet::new(), None);
        assert!(result.diagnostics.iter().any(|d| d.code == "S031" && d.message.contains("2")));
    }

    fn diagram_schema() -> Schema {
        Schema::from_str(
            r#"
type "doc" {
    field "title" type="string"
    section "Arch" required=#true {
        diagram type="mermaid"
    }
}
"#,
        )
        .unwrap()
    }

    #[test]
    fn test_diagram_constraint_pass() {
        let doc = Document::from_str(
            "---\ntype: doc\ntitle: T\n---\n\n# Arch\n\n```mermaid\ngraph TD\n  A-->B\n```\n",
        )
        .unwrap();
        let schema = diagram_schema();
        let result = validate_document(&doc, &schema, &HashSet::new(), &HashSet::new(), None);
        assert_eq!(result.errors(), 0, "diagnostics: {:?}", result.diagnostics);
    }

    #[test]
    fn test_diagram_constraint_missing() {
        let doc = Document::from_str(
            "---\ntype: doc\ntitle: T\n---\n\n# Arch\n\nJust text.\n",
        )
        .unwrap();
        let schema = diagram_schema();
        let result = validate_document(&doc, &schema, &HashSet::new(), &HashSet::new(), None);
        assert!(result.diagnostics.iter().any(|d| d.code == "S032"));
    }

    #[test]
    fn test_diagram_constraint_wrong_type() {
        let doc = Document::from_str(
            "---\ntype: doc\ntitle: T\n---\n\n# Arch\n\n```d2\nshape: oval\n```\n",
        )
        .unwrap();
        let schema = diagram_schema();
        let result = validate_document(&doc, &schema, &HashSet::new(), &HashSet::new(), None);
        assert!(result.diagnostics.iter().any(|d| d.code == "S032"));
    }

    #[test]
    fn test_diagram_any_type() {
        let schema = Schema::from_str(
            r#"
type "doc" {
    field "title" type="string"
    section "Arch" required=#true {
        diagram
    }
}
"#,
        )
        .unwrap();
        // d2 should pass with "any" diagram type
        let doc = Document::from_str(
            "---\ntype: doc\ntitle: T\n---\n\n# Arch\n\n```d2\nshape: oval\n```\n",
        )
        .unwrap();
        let result = validate_document(&doc, &schema, &HashSet::new(), &HashSet::new(), None);
        assert_eq!(result.errors(), 0, "diagnostics: {:?}", result.diagnostics);
    }

    #[test]
    fn test_description_enriches_field_hint() {
        let schema = Schema::from_str(
            r#"
type "doc" {
    field "title" type="string" required=#true description="Short summary"
    section "Body" required=#true
}
"#,
        )
        .unwrap();
        let doc = Document::from_str(
            "---\ntype: doc\n---\n\n# Body\n\nContent\n",
        )
        .unwrap();
        let result = validate_document(&doc, &schema, &HashSet::new(), &HashSet::new(), None);
        let f010 = result.diagnostics.iter().find(|d| d.code == "F010").unwrap();
        assert!(f010.hint.as_ref().unwrap().contains("Short summary"));
    }

    // ─── Conditional rule tests ──────────────────────────────────────────

    fn rule_schema() -> Schema {
        Schema::from_str(
            r#"
type "adr" {
    field "status" type="enum" required=#true {
        values "proposed" "accepted" "superseded"
    }
    field "date" type="string"
    field "superseded_by" type="string"
    section "Decision" required=#true

    rule "accepted requires date" {
        when "status" equals="accepted"
        then-required "date"
    }
    rule "superseded requires superseded_by" {
        when "status" equals="superseded"
        then-required "superseded_by"
    }
}
"#,
        )
        .unwrap()
    }

    #[test]
    fn test_rule_condition_not_triggered() {
        let doc = Document::from_str(
            "---\ntype: adr\nstatus: proposed\n---\n\n# Decision\n\nX\n",
        )
        .unwrap();
        let schema = rule_schema();
        let result = validate_document(&doc, &schema, &HashSet::new(), &HashSet::new(), None);
        assert!(
            !result.diagnostics.iter().any(|d| d.code == "F040"),
            "should not trigger rule when condition doesn't match"
        );
    }

    #[test]
    fn test_rule_condition_met_field_present() {
        let doc = Document::from_str(
            "---\ntype: adr\nstatus: accepted\ndate: \"2025-01-01\"\n---\n\n# Decision\n\nX\n",
        )
        .unwrap();
        let schema = rule_schema();
        let result = validate_document(&doc, &schema, &HashSet::new(), &HashSet::new(), None);
        assert!(
            !result.diagnostics.iter().any(|d| d.code == "F040"),
            "should not error when conditionally required field is present"
        );
    }

    #[test]
    fn test_rule_condition_met_field_missing() {
        let doc = Document::from_str(
            "---\ntype: adr\nstatus: accepted\n---\n\n# Decision\n\nX\n",
        )
        .unwrap();
        let schema = rule_schema();
        let result = validate_document(&doc, &schema, &HashSet::new(), &HashSet::new(), None);
        let f040s: Vec<_> = result.diagnostics.iter().filter(|d| d.code == "F040").collect();
        assert_eq!(f040s.len(), 1, "expected 1 F040 diagnostic, got: {:?}", f040s);
        assert!(f040s[0].message.contains("date"));
        assert!(f040s[0].message.contains("status=accepted"));
    }

    #[test]
    fn test_rule_superseded_missing_field() {
        let doc = Document::from_str(
            "---\ntype: adr\nstatus: superseded\n---\n\n# Decision\n\nX\n",
        )
        .unwrap();
        let schema = rule_schema();
        let result = validate_document(&doc, &schema, &HashSet::new(), &HashSet::new(), None);
        let f040s: Vec<_> = result.diagnostics.iter().filter(|d| d.code == "F040").collect();
        assert_eq!(f040s.len(), 1);
        assert!(f040s[0].message.contains("superseded_by"));
    }

    #[test]
    fn test_rule_superseded_field_present() {
        let doc = Document::from_str(
            "---\ntype: adr\nstatus: superseded\nsuperseded_by: ADR-002\n---\n\n# Decision\n\nX\n",
        )
        .unwrap();
        let schema = rule_schema();
        let result = validate_document(&doc, &schema, &HashSet::new(), &HashSet::new(), None);
        assert!(
            !result.diagnostics.iter().any(|d| d.code == "F040"),
            "should pass when superseded_by is present"
        );
    }

    #[test]
    fn test_description_enriches_section_hint() {
        let schema = Schema::from_str(
            r#"
type "doc" {
    field "title" type="string"
    section "Decision" required=#true description="The decision and rationale"
}
"#,
        )
        .unwrap();
        let doc = Document::from_str(
            "---\ntype: doc\ntitle: T\n---\n\n# Other\n\nStuff\n",
        )
        .unwrap();
        let result = validate_document(&doc, &schema, &HashSet::new(), &HashSet::new(), None);
        let s010 = result.diagnostics.iter().find(|d| d.code == "S010").unwrap();
        assert!(s010.hint.as_ref().unwrap().contains("The decision and rationale"));
    }
}
