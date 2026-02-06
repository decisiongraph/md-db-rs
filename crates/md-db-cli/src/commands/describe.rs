use std::path::PathBuf;

use clap::Args;
use md_db::schema::{Cardinality, FieldType, Schema};

#[derive(Debug, Args)]
pub struct DescribeArgs {
    /// Path to KDL schema file
    #[arg(long)]
    pub schema: PathBuf,

    /// Show details for a specific type
    #[arg(long = "type")]
    pub doc_type: Option<String>,

    /// Show details for a specific field (requires --type)
    #[arg(long)]
    pub field: Option<String>,

    /// Show all relations
    #[arg(long)]
    pub relations: bool,

    /// Export full schema as JSON (all types expanded)
    #[arg(long)]
    pub export: bool,

    /// Output format: text, json
    #[arg(long, default_value = "text")]
    pub format: String,
}

pub fn run(args: &DescribeArgs) -> Result<(), Box<dyn std::error::Error>> {
    let schema = Schema::from_file(&args.schema)?;

    let json_mode = args.format == "json";

    if args.export {
        let full = export_schema_json(&schema);
        println!("{}", serde_json::to_string_pretty(&full)?);
        return Ok(());
    }

    if args.relations {
        if json_mode {
            println!("{}", serde_json::to_string_pretty(&relations_to_json(&schema))?);
        } else {
            print_relations(&schema);
        }
        return Ok(());
    }

    if let Some(ref type_name) = args.doc_type {
        let type_def = schema
            .get_type(type_name)
            .ok_or_else(|| format!("unknown type \"{type_name}\""))?;

        if let Some(ref field_name) = args.field {
            let field_def = type_def
                .fields
                .iter()
                .find(|f| f.name == *field_name)
                .ok_or_else(|| format!("unknown field \"{field_name}\" in type \"{type_name}\""))?;

            if json_mode {
                println!("{}", serde_json::to_string_pretty(&field_to_json(field_def))?);
            } else {
                print_field_detail(field_def);
            }
        } else {
            if json_mode {
                println!("{}", serde_json::to_string_pretty(&type_to_json(type_def, &schema))?);
            } else {
                print_type_detail(type_def, &schema);
            }
        }
    } else {
        // Overview: list all types + relations summary
        if json_mode {
            println!("{}", serde_json::to_string_pretty(&overview_to_json(&schema))?);
        } else {
            print_overview(&schema);
        }
    }

    Ok(())
}

// ─── Text output ─────────────────────────────────────────────────────────────

fn print_overview(schema: &Schema) {
    println!("Types:");
    for t in &schema.types {
        let desc = t
            .description
            .as_ref()
            .map(|d| format!(" — {d}"))
            .unwrap_or_default();
        let mut meta = Vec::new();
        if let Some(ref f) = t.folder {
            meta.push(format!("folder={f}"));
        }
        if let Some(m) = t.max_count {
            meta.push(format!("max_count={m}"));
        }
        let meta_str = if meta.is_empty() {
            String::new()
        } else {
            format!("  ({})", meta.join(", "))
        };
        println!("  {}{desc}{meta_str}", t.name);
    }

    if !schema.relations.is_empty() {
        println!("\nRelations:");
        for r in &schema.relations {
            let inv = r
                .inverse
                .as_ref()
                .map(|i| format!(" → {i}"))
                .unwrap_or_default();
            let card = match r.cardinality {
                Cardinality::One => "one",
                Cardinality::Many => "many",
            };
            let desc = r
                .description
                .as_ref()
                .map(|d| format!("  {d}"))
                .unwrap_or_default();
            println!("  {}{inv}  ({card}){desc}", r.name);
        }
    }
}

fn print_type_detail(type_def: &md_db::schema::TypeDef, schema: &Schema) {
    let desc = type_def
        .description
        .as_ref()
        .map(|d| format!(" — {d}"))
        .unwrap_or_default();
    println!("Type: {}{desc}", type_def.name);

    if let Some(ref folder) = type_def.folder {
        println!("  folder: {folder}");
    }
    if let Some(max) = type_def.max_count {
        println!("  max_count: {max}");
    }

    if !type_def.fields.is_empty() {
        println!("\nFields:");
        for f in &type_def.fields {
            let req = if f.required { "required" } else { "" };
            let type_str = field_type_short(&f.field_type);
            let desc = f
                .description
                .as_ref()
                .map(|d| format!("  {d}"))
                .unwrap_or_default();
            println!("  {:<14}{:<9}{:<10}{desc}", f.name, type_str, req);

            // Extra details on indented lines
            if let FieldType::Enum(ref vals) = f.field_type {
                println!("{:>35}values: {}", "", vals.join(", "));
            }
            if let Some(ref pat) = f.pattern {
                println!("{:>35}pattern: {pat}", "");
            }
            if let Some(ref def) = f.default {
                println!("{:>35}default: {def}", "");
            }
        }
    }

    if !type_def.sections.is_empty() {
        println!("\nSections:");
        print_section_tree(&type_def.sections, 1);
    }

    // Conditional rules
    if !type_def.rules.is_empty() {
        println!("\nRules:");
        for r in &type_def.rules {
            println!(
                "  \"{}\"  when {}={} -> require {}",
                r.name,
                r.when_field,
                r.when_equals,
                r.then_required.join(", ")
            );
        }
    }

    // Relations that apply to all types
    if !schema.relations.is_empty() {
        println!("\nRelations (all types):");
        for r in &schema.relations {
            let inv = r
                .inverse
                .as_ref()
                .map(|i| format!(" → {i}"))
                .unwrap_or_default();
            let card = match r.cardinality {
                Cardinality::One => "one",
                Cardinality::Many => "many",
            };
            let desc = r
                .description
                .as_ref()
                .map(|d| format!("  {d}"))
                .unwrap_or_default();
            println!("  {}{inv}  ({card}){desc}", r.name);
        }
    }
}

fn print_section_tree(sections: &[md_db::schema::SectionDef], depth: usize) {
    for s in sections {
        let prefix: String = "#".repeat(depth);
        let req = if s.required { "required" } else { "" };
        let desc = s
            .description
            .as_ref()
            .map(|d| format!("  {d}"))
            .unwrap_or_default();
        println!("  {prefix} {:<20}{:<10}{desc}", s.name, req);

        // Content constraints
        if let Some(ref c) = s.content {
            let min = c
                .min_paragraphs
                .map(|n| format!("min {n} paragraph(s)"))
                .unwrap_or_else(|| "prose".into());
            println!("{:>35}content: {min}", "");
        }
        if let Some(ref l) = s.list {
            let detail = l
                .min_items
                .map(|n| format!("min {n} item(s)"))
                .unwrap_or_else(|| "required".into());
            println!("{:>35}list: {detail}", "");
        }
        if let Some(ref d) = s.diagram {
            let detail = d
                .diagram_type
                .as_ref()
                .map(|t| format!("type={t}"))
                .unwrap_or_else(|| "any".into());
            println!("{:>35}diagram: {detail}", "");
        }
        if let Some(ref t) = s.table {
            let cols: Vec<&str> = t.columns.iter().map(|c| c.name.as_str()).collect();
            let desc = t
                .description
                .as_ref()
                .map(|d| format!("  {d}"))
                .unwrap_or_default();
            println!("{:>35}table: {}{desc}", "", cols.join(" | "));
        }

        if !s.children.is_empty() {
            print_section_tree(&s.children, depth + 1);
        }
    }
}

fn print_field_detail(field_def: &md_db::schema::FieldDef) {
    println!("Field: {}", field_def.name);
    println!("  type: {}", field_def.field_type);
    println!("  required: {}", field_def.required);
    if let Some(ref desc) = field_def.description {
        println!("  description: {desc}");
    }
    if let Some(ref pat) = field_def.pattern {
        println!("  pattern: {pat}");
    }
    if let Some(ref def) = field_def.default {
        println!("  default: {def}");
    }
    if let FieldType::Enum(ref vals) = field_def.field_type {
        println!("  values: {}", vals.join(", "));
    }
}

fn print_relations(schema: &Schema) {
    if schema.relations.is_empty() {
        println!("No relations defined.");
        return;
    }
    println!("Relations:");
    for r in &schema.relations {
        let inv = r
            .inverse
            .as_ref()
            .map(|i| format!(" → {i}"))
            .unwrap_or_default();
        let card = match r.cardinality {
            Cardinality::One => "one",
            Cardinality::Many => "many",
        };
        let desc = r
            .description
            .as_ref()
            .map(|d| format!("\n    {d}"))
            .unwrap_or_default();
        println!("  {}{inv}  ({card}){desc}", r.name);
    }
}

fn field_type_short(ft: &FieldType) -> String {
    match ft {
        FieldType::String => "string".into(),
        FieldType::Number => "number".into(),
        FieldType::Bool => "bool".into(),
        FieldType::Enum(_) => "enum".into(),
        FieldType::Ref => "ref".into(),
        FieldType::StringArray => "string[]".into(),
        FieldType::RefArray => "ref[]".into(),
        FieldType::User => "user".into(),
        FieldType::UserArray => "user[]".into(),
    }
}

// ─── JSON output ─────────────────────────────────────────────────────────────

fn overview_to_json(schema: &Schema) -> serde_json::Value {
    let types: Vec<serde_json::Value> = schema
        .types
        .iter()
        .map(|t| {
            let mut obj = serde_json::json!({
                "name": t.name,
                "description": t.description,
                "fields": t.fields.len(),
                "sections": t.sections.len(),
            });
            if let Some(ref f) = t.folder {
                obj["folder"] = serde_json::Value::String(f.clone());
            }
            if let Some(m) = t.max_count {
                obj["max_count"] = serde_json::json!(m);
            }
            obj
        })
        .collect();

    serde_json::json!({
        "types": types,
        "relations": relations_to_json(schema),
    })
}

fn type_to_json(
    type_def: &md_db::schema::TypeDef,
    schema: &Schema,
) -> serde_json::Value {
    let fields: Vec<serde_json::Value> = type_def
        .fields
        .iter()
        .map(|f| field_to_json(f))
        .collect();

    let sections: Vec<serde_json::Value> = type_def
        .sections
        .iter()
        .map(|s| section_to_json(s))
        .collect();

    let rules: Vec<serde_json::Value> = type_def
        .rules
        .iter()
        .map(|r| {
            serde_json::json!({
                "name": r.name,
                "when_field": r.when_field,
                "when_equals": r.when_equals,
                "then_required": r.then_required,
            })
        })
        .collect();

    let mut obj = serde_json::json!({
        "name": type_def.name,
        "description": type_def.description,
        "fields": fields,
        "sections": sections,
        "rules": rules,
        "relations": relations_to_json(schema),
    });
    if let Some(ref f) = type_def.folder {
        obj["folder"] = serde_json::Value::String(f.clone());
    }
    if let Some(m) = type_def.max_count {
        obj["max_count"] = serde_json::json!(m);
    }
    obj
}

fn field_to_json(f: &md_db::schema::FieldDef) -> serde_json::Value {
    let mut obj = serde_json::json!({
        "name": f.name,
        "type": field_type_short(&f.field_type),
        "required": f.required,
    });
    if let Some(ref desc) = f.description {
        obj["description"] = serde_json::Value::String(desc.clone());
    }
    if let Some(ref pat) = f.pattern {
        obj["pattern"] = serde_json::Value::String(pat.clone());
    }
    if let Some(ref def) = f.default {
        obj["default"] = serde_json::Value::String(def.clone());
    }
    if let FieldType::Enum(ref vals) = f.field_type {
        obj["values"] = serde_json::json!(vals);
    }
    obj
}

fn section_to_json(s: &md_db::schema::SectionDef) -> serde_json::Value {
    let mut obj = serde_json::json!({
        "name": s.name,
        "required": s.required,
    });
    if let Some(ref desc) = s.description {
        obj["description"] = serde_json::Value::String(desc.clone());
    }
    if let Some(ref c) = s.content {
        obj["content"] = serde_json::json!({ "min_paragraphs": c.min_paragraphs });
    }
    if let Some(ref l) = s.list {
        obj["list"] = serde_json::json!({ "required": l.required, "min_items": l.min_items });
    }
    if let Some(ref d) = s.diagram {
        obj["diagram"] = serde_json::json!({ "required": d.required, "type": d.diagram_type });
    }
    if let Some(ref t) = s.table {
        let cols: Vec<serde_json::Value> = t
            .columns
            .iter()
            .map(|c| {
                let mut col = serde_json::json!({
                    "name": c.name,
                    "type": field_type_short(&c.col_type),
                    "required": c.required,
                });
                if let Some(ref desc) = c.description {
                    col["description"] = serde_json::Value::String(desc.clone());
                }
                col
            })
            .collect();
        let mut table_obj = serde_json::json!({ "required": t.required, "columns": cols });
        if let Some(ref desc) = t.description {
            table_obj["description"] = serde_json::Value::String(desc.clone());
        }
        obj["table"] = table_obj;
    }
    if !s.children.is_empty() {
        let children: Vec<serde_json::Value> =
            s.children.iter().map(|c| section_to_json(c)).collect();
        obj["children"] = serde_json::json!(children);
    }
    obj
}

/// Full schema export: all types with all fields/sections/constraints expanded.
fn export_schema_json(schema: &Schema) -> serde_json::Value {
    let types: Vec<serde_json::Value> = schema
        .types
        .iter()
        .map(|t| {
            let fields: Vec<serde_json::Value> =
                t.fields.iter().map(|f| field_to_json(f)).collect();
            let sections: Vec<serde_json::Value> =
                t.sections.iter().map(|s| section_to_json(s)).collect();
            let rules: Vec<serde_json::Value> = t
                .rules
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "name": r.name,
                        "when_field": r.when_field,
                        "when_equals": r.when_equals,
                        "then_required": r.then_required,
                    })
                })
                .collect();
            let mut obj = serde_json::json!({
                "name": t.name,
                "description": t.description,
                "fields": fields,
                "sections": sections,
                "rules": rules,
            });
            if let Some(ref f) = t.folder {
                obj["folder"] = serde_json::Value::String(f.clone());
            }
            if let Some(m) = t.max_count {
                obj["max_count"] = serde_json::json!(m);
            }
            obj
        })
        .collect();

    let ref_formats: Vec<serde_json::Value> = schema
        .ref_formats
        .iter()
        .map(|rf| {
            serde_json::json!({
                "name": rf.name,
                "pattern": rf.pattern,
            })
        })
        .collect();

    serde_json::json!({
        "types": types,
        "relations": relations_to_json(schema),
        "ref_formats": ref_formats,
    })
}

fn relations_to_json(schema: &Schema) -> serde_json::Value {
    let rels: Vec<serde_json::Value> = schema
        .relations
        .iter()
        .map(|r| {
            let mut obj = serde_json::json!({
                "name": r.name,
                "inverse": r.inverse,
                "cardinality": match r.cardinality {
                    Cardinality::One => "one",
                    Cardinality::Many => "many",
                },
            });
            if let Some(ref desc) = r.description {
                obj["description"] = serde_json::Value::String(desc.clone());
            }
            obj
        })
        .collect();
    serde_json::json!(rels)
}
