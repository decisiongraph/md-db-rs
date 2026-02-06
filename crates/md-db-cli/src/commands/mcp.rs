//! Minimal MCP (Model Context Protocol) server over stdio.
//!
//! Reads JSON-RPC 2.0 requests line-by-line from stdin, dispatches to md-db
//! library functions, and writes JSON-RPC responses to stdout.

use std::collections::HashSet;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use md_db::discovery::{self, Filter};
use md_db::document::Document;
use md_db::frontmatter::Frontmatter;
use md_db::graph::{DocGraph, path_to_id};
use md_db::output;
use md_db::schema::Schema;
use md_db::template;
use md_db::users::UserConfig;
use md_db::validation;

use serde_json::{json, Value};

// ── Tool descriptors ────────────────────────────────────────────────────────

fn tool_list() -> Value {
    json!([
        {
            "name": "md-db-validate",
            "description": "Validate markdown documents against a KDL schema. Returns diagnostics.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "dir":     { "type": "string", "description": "Directory to validate" },
                    "schema":  { "type": "string", "description": "Path to KDL schema file" },
                    "file":    { "type": "string", "description": "Single file to validate (instead of dir)" },
                    "pattern": { "type": "string", "description": "Glob pattern (default *.md)" },
                    "users":   { "type": "string", "description": "Path to user/team config YAML" }
                },
                "required": ["schema"]
            }
        },
        {
            "name": "md-db-get",
            "description": "Read a field, section, table, or cell from a markdown document.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file":        { "type": "string",  "description": "Path to the markdown file" },
                    "field":       { "type": "string",  "description": "Frontmatter field key (dotted paths supported)" },
                    "frontmatter": { "type": "boolean", "description": "Return full frontmatter" },
                    "section":     { "type": "string",  "description": "Section heading" },
                    "table":       { "type": "integer", "description": "Table index within section (0-based)" },
                    "cell":        { "type": "string",  "description": "Cell spec: Column,Row" }
                },
                "required": ["file"]
            }
        },
        {
            "name": "md-db-list",
            "description": "List and filter markdown documents by frontmatter fields.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "dir":     { "type": "string", "description": "Directory to search" },
                    "pattern": { "type": "string", "description": "Glob pattern (default *.md)" },
                    "fields":  { "type": "array",  "items": { "type": "string" }, "description": "Filters: key=value" },
                    "sort":    { "type": "string", "description": "Sort by field (prefix - for descending)" }
                },
                "required": ["dir"]
            }
        },
        {
            "name": "md-db-inspect",
            "description": "Inspect a document: frontmatter, sections, validation diagnostics.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file":   { "type": "string", "description": "Path to the markdown file" },
                    "schema": { "type": "string", "description": "Path to KDL schema file" },
                    "users":  { "type": "string", "description": "Path to user/team config YAML" }
                },
                "required": ["file", "schema"]
            }
        },
        {
            "name": "md-db-describe",
            "description": "Describe schema types, fields, sections, and relations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "schema":    { "type": "string",  "description": "Path to KDL schema file" },
                    "type":      { "type": "string",  "description": "Show details for a specific type" },
                    "field":     { "type": "string",  "description": "Show details for a field (requires type)" },
                    "relations": { "type": "boolean", "description": "Show all relations" },
                    "export":    { "type": "boolean", "description": "Export full schema as JSON" }
                },
                "required": ["schema"]
            }
        },
        {
            "name": "md-db-set",
            "description": "Set/update fields, sections, or table cells in a markdown document.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file":         { "type": "string",  "description": "Path to the markdown file" },
                    "fields":       { "type": "array",   "items": { "type": "string" }, "description": "Field updates: key=value" },
                    "section":      { "type": "string",  "description": "Target section heading" },
                    "content":      { "type": "string",  "description": "Replace section content" },
                    "append":       { "type": "string",  "description": "Append to section" },
                    "table":        { "type": "integer", "description": "Table index (0-based)" },
                    "cell":         { "type": "string",  "description": "Cell spec: Column,Row" },
                    "value":        { "type": "string",  "description": "Value for --cell" },
                    "add_row":      { "type": "string",  "description": "Add row (comma-separated)" },
                    "section_sets": { "type": "array",   "items": { "type": "string" }, "description": "Batch: Heading=content" },
                    "dry_run":      { "type": "boolean", "description": "Return result without writing" }
                },
                "required": ["file"]
            }
        },
        {
            "name": "md-db-new",
            "description": "Create a new document from a schema type definition.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "type":    { "type": "string",  "description": "Document type name" },
                    "schema":  { "type": "string",  "description": "Path to KDL schema file" },
                    "output":  { "type": "string",  "description": "Output file path" },
                    "dir":     { "type": "string",  "description": "Directory for auto-ID" },
                    "fields":  { "type": "array",   "items": { "type": "string" }, "description": "Pre-fill: key=value" },
                    "fill":    { "type": "boolean", "description": "Expand template variables" },
                    "auto_id": { "type": "boolean", "description": "Auto-generate path using next ID" }
                },
                "required": ["type", "schema"]
            }
        },
        {
            "name": "md-db-refs",
            "description": "Show forward refs or backlinks for a document.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "dir":    { "type": "string",  "description": "Directory containing markdown files" },
                    "schema": { "type": "string",  "description": "Path to KDL schema file" },
                    "from":   { "type": "string",  "description": "Show outgoing refs from this ID/file" },
                    "to":     { "type": "string",  "description": "Show backlinks to this ID" },
                    "depth":  { "type": "integer", "description": "Transitive depth (default 1)" }
                },
                "required": ["dir", "schema"]
            }
        },
        {
            "name": "md-db-graph",
            "description": "Export the document link graph as JSON.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "dir":    { "type": "string", "description": "Directory containing markdown files" },
                    "schema": { "type": "string", "description": "Path to KDL schema file" },
                    "type":   { "type": "string", "description": "Filter by document type" }
                },
                "required": ["dir", "schema"]
            }
        },
        {
            "name": "md-db-deprecate",
            "description": "Mark a document as deprecated or superseded.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file":          { "type": "string",  "description": "Path to the markdown file" },
                    "schema":        { "type": "string",  "description": "Path to KDL schema file" },
                    "superseded_by": { "type": "string",  "description": "Replacement document ID" },
                    "dir":           { "type": "string",  "description": "Directory for backlink scanning" },
                    "dry_run":       { "type": "boolean", "description": "Print result without writing" }
                },
                "required": ["file", "schema"]
            }
        }
    ])
}

// ── JSON-RPC helpers ────────────────────────────────────────────────────────

fn jsonrpc_ok(id: &Value, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    })
}

fn jsonrpc_error(id: &Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
}

fn text_content(text: &str) -> Value {
    json!([{ "type": "text", "text": text }])
}

// ── Tool dispatch ───────────────────────────────────────────────────────────

fn handle_tool_call(name: &str, args: &Value) -> Result<Value, String> {
    match name {
        "md-db-validate" => tool_validate(args),
        "md-db-get" => tool_get(args),
        "md-db-list" => tool_list_docs(args),
        "md-db-inspect" => tool_inspect(args),
        "md-db-describe" => tool_describe(args),
        "md-db-set" => tool_set(args),
        "md-db-new" => tool_new(args),
        "md-db-refs" => tool_refs(args),
        "md-db-graph" => tool_graph(args),
        "md-db-deprecate" => tool_deprecate(args),
        _ => Err(format!("unknown tool: {name}")),
    }
}

fn str_arg(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
}

fn bool_arg(args: &Value, key: &str) -> bool {
    args.get(key).and_then(|v| v.as_bool()).unwrap_or(false)
}

fn int_arg(args: &Value, key: &str) -> Option<usize> {
    args.get(key).and_then(|v| v.as_u64()).map(|n| n as usize)
}

fn str_array_arg(args: &Value, key: &str) -> Vec<String> {
    args.get(key)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

fn require_str(args: &Value, key: &str) -> Result<String, String> {
    str_arg(args, key).ok_or_else(|| format!("missing required argument: {key}"))
}

// ── Tool implementations ────────────────────────────────────────────────────

fn tool_validate(args: &Value) -> Result<Value, String> {
    let schema_path = require_str(args, "schema")?;
    let schema = Schema::from_file(&PathBuf::from(&schema_path)).map_err(|e| e.to_string())?;
    let user_config = str_arg(args, "users")
        .map(|p| UserConfig::from_file(&PathBuf::from(p)))
        .transpose()
        .map_err(|e| e.to_string())?;
    let pattern = str_arg(args, "pattern");

    let result = if let Some(file) = str_arg(args, "file") {
        let content =
            std::fs::read_to_string(&file).map_err(|e| format!("read {file}: {e}"))?;
        let doc = Document::from_str(&content).map_err(|e| e.to_string())?;
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
    } else if let Some(dir) = str_arg(args, "dir") {
        validation::validate_directory(
            &PathBuf::from(&dir),
            &schema,
            pattern.as_deref(),
            user_config.as_ref(),
        )
        .map_err(|e| e.to_string())?
    } else {
        return Err("provide 'dir' or 'file'".into());
    };

    let out = validate_result_to_json(&result);
    Ok(json!(out))
}

fn validate_result_to_json(result: &validation::ValidationResult) -> Value {
    let files: Vec<Value> = result
        .file_results
        .iter()
        .filter(|f| !f.diagnostics.is_empty())
        .map(|f| {
            let diags: Vec<Value> = f
                .diagnostics
                .iter()
                .map(|d| {
                    json!({
                        "severity": d.severity.to_string(),
                        "code": d.code,
                        "message": d.message,
                        "location": d.location,
                        "hint": d.hint,
                    })
                })
                .collect();
            json!({ "path": f.path, "diagnostics": diags })
        })
        .collect();

    json!({
        "files": files,
        "errors": result.total_errors(),
        "warnings": result.total_warnings(),
        "ok": result.is_ok(),
    })
}

fn tool_get(args: &Value) -> Result<Value, String> {
    let file = require_str(args, "file")?;
    let doc = Document::from_file(&PathBuf::from(&file)).map_err(|e| e.to_string())?;

    if let Some(field_key) = str_arg(args, "field") {
        let fm = doc.frontmatter().map_err(|e| e.to_string())?;
        let val = fm
            .get(&field_key)
            .ok_or_else(|| format!("field not found: {field_key}"))?;
        return Ok(json!({
            "field": field_key,
            "value": output::format_field_value(val, output::OutputFormat::Text),
        }));
    }

    if bool_arg(args, "frontmatter") {
        let fm = doc.frontmatter().map_err(|e| e.to_string())?;
        return Ok(fm.to_json());
    }

    if let Some(heading) = str_arg(args, "section") {
        let section = doc.get_section(&heading).map_err(|e| e.to_string())?;

        if let Some(table_idx) = int_arg(args, "table") {
            let tables = section.tables();
            let table = tables
                .get(table_idx)
                .ok_or_else(|| format!("table index {table_idx} not found"))?;

            if let Some(cell_spec) = str_arg(args, "cell") {
                let (col, row) = parse_cell_spec(&cell_spec)?;
                let val = table
                    .get_cell_or_err(&col, row)
                    .map_err(|e| e.to_string())?;
                return Ok(json!({ "cell": cell_spec, "value": val }));
            }

            return Ok(json!({
                "table_index": table_idx,
                "markdown": output::format_table(table, output::OutputFormat::Markdown),
            }));
        }

        return Ok(json!({
            "heading": section.heading.trim(),
            "level": section.level,
            "content": section.content,
        }));
    }

    // Full document
    Ok(doc.to_json())
}

fn tool_list_docs(args: &Value) -> Result<Value, String> {
    let dir = require_str(args, "dir")?;
    let pattern = str_arg(args, "pattern");
    let field_filters = str_array_arg(args, "fields");

    let mut filters = Vec::new();
    for f in &field_filters {
        if let Some((key, value)) = f.split_once('=') {
            filters.push(Filter::FieldEquals {
                key: key.to_string(),
                value: value.to_string(),
            });
        }
    }

    let mut files =
        discovery::discover_files(&PathBuf::from(&dir), pattern.as_deref(), &filters, false)
            .map_err(|e| e.to_string())?;

    // Sort if requested
    if let Some(sort_spec) = str_arg(args, "sort") {
        let (sort_key, descending) = if let Some(key) = sort_spec.strip_prefix('-') {
            (key.to_string(), true)
        } else {
            (sort_spec, false)
        };

        let mut file_vals: Vec<(PathBuf, Option<String>)> = files
            .into_iter()
            .map(|path| {
                let val = std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|content| Frontmatter::try_parse(&content).ok())
                    .and_then(|(fm, _)| fm)
                    .and_then(|fm| fm.get_display(&sort_key));
                (path, val)
            })
            .collect();

        file_vals.sort_by(|a, b| {
            let cmp = a
                .1
                .as_deref()
                .unwrap_or("")
                .cmp(b.1.as_deref().unwrap_or(""));
            if descending {
                cmp.reverse()
            } else {
                cmp
            }
        });

        files = file_vals.into_iter().map(|(path, _)| path).collect();
    }

    let entries: Vec<Value> = files
        .iter()
        .map(|path| {
            let fm_json = std::fs::read_to_string(path)
                .ok()
                .and_then(|content| Frontmatter::try_parse(&content).ok())
                .and_then(|(fm, _)| fm.map(|f| f.to_json()));
            json!({
                "path": path.display().to_string(),
                "frontmatter": fm_json,
            })
        })
        .collect();

    Ok(json!({ "files": entries, "count": entries.len() }))
}

fn tool_inspect(args: &Value) -> Result<Value, String> {
    let file = require_str(args, "file")?;
    let schema_path = require_str(args, "schema")?;
    let schema = Schema::from_file(&PathBuf::from(&schema_path)).map_err(|e| e.to_string())?;
    let user_config = str_arg(args, "users")
        .map(|p| UserConfig::from_file(&PathBuf::from(p)))
        .transpose()
        .map_err(|e| e.to_string())?;

    let doc = Document::from_file(&PathBuf::from(&file)).map_err(|e| e.to_string())?;

    let file_result = validation::validate_document(
        &doc,
        &schema,
        &HashSet::new(),
        &HashSet::new(),
        user_config.as_ref(),
    );

    let frontmatter = doc
        .frontmatter
        .as_ref()
        .map(|fm| fm.to_json())
        .unwrap_or(Value::Null);

    let sections: Vec<Value> = doc
        .sections()
        .iter()
        .map(|s| {
            json!({
                "heading": s.heading.trim(),
                "level": s.level,
                "content_length": s.content.len(),
            })
        })
        .collect();

    let diagnostics: Vec<Value> = file_result
        .diagnostics
        .iter()
        .map(|d| {
            json!({
                "severity": d.severity.to_string(),
                "code": d.code,
                "message": d.message,
                "location": d.location,
                "hint": d.hint,
            })
        })
        .collect();

    Ok(json!({
        "path": file,
        "frontmatter": frontmatter,
        "sections": sections,
        "diagnostics": diagnostics,
        "errors": file_result.errors(),
        "warnings": file_result.warnings(),
        "valid": file_result.errors() == 0,
    }))
}

fn tool_describe(args: &Value) -> Result<Value, String> {
    let schema_path = require_str(args, "schema")?;
    let schema = Schema::from_file(&PathBuf::from(&schema_path)).map_err(|e| e.to_string())?;

    if bool_arg(args, "export") {
        return Ok(export_schema_json(&schema));
    }

    if bool_arg(args, "relations") {
        return Ok(relations_to_json(&schema));
    }

    if let Some(type_name) = str_arg(args, "type") {
        let type_def = schema
            .get_type(&type_name)
            .ok_or_else(|| format!("unknown type: {type_name}"))?;

        if let Some(field_name) = str_arg(args, "field") {
            let field_def = type_def
                .fields
                .iter()
                .find(|f| f.name == field_name)
                .ok_or_else(|| format!("unknown field: {field_name}"))?;
            return Ok(field_to_json(field_def));
        }

        return Ok(type_to_json(type_def));
    }

    // Overview
    let types: Vec<Value> = schema
        .types
        .iter()
        .map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "fields": t.fields.len(),
                "sections": t.sections.len(),
                "folder": t.folder,
                "max_count": t.max_count,
            })
        })
        .collect();

    Ok(json!({
        "types": types,
        "relations": relations_to_json(&schema),
    }))
}

fn tool_set(args: &Value) -> Result<Value, String> {
    let file = require_str(args, "file")?;
    let dry_run = bool_arg(args, "dry_run");
    let mut doc = Document::from_file(&PathBuf::from(&file)).map_err(|e| e.to_string())?;

    for field_str in str_array_arg(args, "fields") {
        let (key, value) = field_str
            .split_once('=')
            .ok_or_else(|| format!("invalid field format: {field_str}"))?;
        doc.set_field_from_str(key, value);
    }

    for ss in str_array_arg(args, "section_sets") {
        let (heading, content) = ss
            .split_once('=')
            .ok_or_else(|| format!("invalid section-set: {ss}"))?;
        doc.replace_section_content(heading.trim(), &format!("{}\n", content.trim()))
            .map_err(|e| e.to_string())?;
    }

    if let Some(heading) = str_arg(args, "section") {
        if let Some(content) = str_arg(args, "content") {
            doc.replace_section_content(&heading, &format!("{content}\n"))
                .map_err(|e| e.to_string())?;
        }
        if let Some(text) = str_arg(args, "append") {
            doc.append_to_section(&heading, &text)
                .map_err(|e| e.to_string())?;
        }
        if let Some(table_idx) = int_arg(args, "table") {
            if let Some(cell_spec) = str_arg(args, "cell") {
                let value = require_str(args, "value")?;
                let (col, row) = parse_cell_spec(&cell_spec)?;
                doc.set_table_cell(&heading, table_idx, &col, row, &value)
                    .map_err(|e| e.to_string())?;
            }
            if let Some(row_str) = str_arg(args, "add_row") {
                let values: Vec<String> = row_str.split(',').map(|s| s.trim().to_string()).collect();
                doc.add_table_row(&heading, table_idx, values)
                    .map_err(|e| e.to_string())?;
            }
        }
    }

    if dry_run {
        Ok(json!({ "content": doc.raw, "written": false }))
    } else {
        doc.save().map_err(|e| e.to_string())?;
        Ok(json!({ "path": file, "written": true }))
    }
}

fn tool_new(args: &Value) -> Result<Value, String> {
    let doc_type = require_str(args, "type")?;
    let schema_path = require_str(args, "schema")?;
    let schema = Schema::from_file(&PathBuf::from(&schema_path)).map_err(|e| e.to_string())?;

    let type_def = schema
        .get_type(&doc_type)
        .ok_or_else(|| format!("unknown type: {doc_type}"))?;

    let field_strs = str_array_arg(args, "fields");
    let fields: Vec<(String, String)> = field_strs
        .iter()
        .map(|s| {
            s.split_once('=')
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .ok_or_else(|| format!("invalid field: {s}"))
        })
        .collect::<Result<_, _>>()?;

    let fill = bool_arg(args, "fill");
    let auto_id = bool_arg(args, "auto_id");

    let output_path = if auto_id {
        let dir = require_str(args, "dir")?;
        let graph =
            DocGraph::build(&PathBuf::from(&dir), &schema).map_err(|e| e.to_string())?;
        let next_id = graph.next_id(&doc_type);
        let folder = type_def.folder.as_deref().unwrap_or(".");
        let filename = format!("{}.md", next_id.to_lowercase());
        Some(PathBuf::from(&dir).join(folder).join(filename))
    } else {
        str_arg(args, "output").map(PathBuf::from)
    };

    let content = template::generate_document_opts(type_def, &schema, &fields, fill);

    if let Some(ref path) = output_path {
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
        }
        std::fs::write(path, &content).map_err(|e| e.to_string())?;
        Ok(json!({ "path": path.display().to_string(), "content": content }))
    } else {
        Ok(json!({ "content": content }))
    }
}

fn tool_refs(args: &Value) -> Result<Value, String> {
    let dir = require_str(args, "dir")?;
    let schema_path = require_str(args, "schema")?;
    let schema = Schema::from_file(&PathBuf::from(&schema_path)).map_err(|e| e.to_string())?;
    let graph =
        DocGraph::build(&PathBuf::from(&dir), &schema).map_err(|e| e.to_string())?;
    let depth = int_arg(args, "depth").unwrap_or(1);

    if let Some(target) = str_arg(args, "to") {
        let id = target.to_uppercase().replace('_', "-");
        let edges = if depth > 1 {
            graph.refs_to_transitive(&id, depth)
        } else {
            graph.refs_to(&id).into_iter().map(|e| (1usize, e)).collect()
        };
        let items: Vec<Value> = edges
            .iter()
            .map(|(d, e)| {
                let node = graph.nodes.get(&e.from);
                json!({
                    "id": e.from,
                    "relation": e.relation,
                    "depth": d,
                    "type": node.and_then(|n| n.doc_type.as_deref()),
                    "title": node.and_then(|n| n.title.as_deref()),
                })
            })
            .collect();
        return Ok(json!({ "id": id, "mode": "backlinks", "results": items, "count": items.len() }));
    }

    if let Some(source) = str_arg(args, "from") {
        let id = if source.contains('/') || source.ends_with(".md") {
            path_to_id(std::path::Path::new(&source))
        } else {
            source.to_uppercase().replace('_', "-")
        };
        let edges = if depth > 1 {
            graph.refs_from_transitive(&id, depth)
        } else {
            graph
                .refs_from(&id)
                .into_iter()
                .map(|e| (1usize, e))
                .collect()
        };
        let items: Vec<Value> = edges
            .iter()
            .map(|(d, e)| {
                let node = graph.nodes.get(&e.to);
                json!({
                    "id": e.to,
                    "relation": e.relation,
                    "depth": d,
                    "type": node.and_then(|n| n.doc_type.as_deref()),
                    "title": node.and_then(|n| n.title.as_deref()),
                })
            })
            .collect();
        return Ok(json!({ "id": id, "mode": "refs", "results": items, "count": items.len() }));
    }

    Err("provide 'from' or 'to'".into())
}

fn tool_graph(args: &Value) -> Result<Value, String> {
    let dir = require_str(args, "dir")?;
    let schema_path = require_str(args, "schema")?;
    let schema = Schema::from_file(&PathBuf::from(&schema_path)).map_err(|e| e.to_string())?;
    let graph =
        DocGraph::build(&PathBuf::from(&dir), &schema).map_err(|e| e.to_string())?;
    let filter_type = str_arg(args, "type");

    let nodes: Vec<Value> = graph
        .nodes
        .values()
        .filter(|n| {
            filter_type
                .as_deref()
                .map(|ft| n.doc_type.as_deref() == Some(ft))
                .unwrap_or(true)
        })
        .map(|n| {
            json!({
                "id": n.id,
                "type": n.doc_type,
                "title": n.title,
                "status": n.status,
                "path": n.path.display().to_string(),
            })
        })
        .collect();

    let edges: Vec<Value> = graph
        .edges
        .iter()
        .map(|e| json!({ "from": e.from, "to": e.to, "relation": e.relation }))
        .collect();

    Ok(json!({
        "nodes": nodes,
        "edges": edges,
        "node_count": nodes.len(),
        "edge_count": edges.len(),
    }))
}

fn tool_deprecate(args: &Value) -> Result<Value, String> {
    let file = require_str(args, "file")?;
    let schema_path = require_str(args, "schema")?;
    let schema = Schema::from_file(&PathBuf::from(&schema_path)).map_err(|e| e.to_string())?;
    let dry_run = bool_arg(args, "dry_run");

    let mut doc = Document::from_file(&PathBuf::from(&file)).map_err(|e| e.to_string())?;
    let doc_id = path_to_id(std::path::Path::new(&file));

    if let Some(replacement) = str_arg(args, "superseded_by") {
        doc.set_field_from_str("status", "superseded");
        doc.set_field_from_str("superseded_by", &replacement);
    } else {
        doc.set_field_from_str("status", "deprecated");
    }

    if dry_run {
        return Ok(json!({ "id": doc_id, "content": doc.raw, "written": false }));
    }

    doc.save().map_err(|e| e.to_string())?;

    let mut backlinks = Vec::new();
    if let Some(dir) = str_arg(args, "dir") {
        let graph =
            DocGraph::build(&PathBuf::from(&dir), &schema).map_err(|e| e.to_string())?;
        for edge in graph.refs_to(&doc_id) {
            if edge.from != doc_id {
                backlinks.push(json!({ "from": edge.from, "relation": edge.relation }));
            }
        }
    }

    Ok(json!({
        "id": doc_id,
        "written": true,
        "backlinks": backlinks,
    }))
}

// ── Schema JSON helpers ─────────────────────────────────────────────────────

fn field_type_short(ft: &md_db::schema::FieldType) -> &'static str {
    use md_db::schema::FieldType;
    match ft {
        FieldType::String => "string",
        FieldType::Number => "number",
        FieldType::Bool => "bool",
        FieldType::Enum(_) => "enum",
        FieldType::Ref => "ref",
        FieldType::StringArray => "string[]",
        FieldType::RefArray => "ref[]",
        FieldType::User => "user",
        FieldType::UserArray => "user[]",
    }
}

fn field_to_json(f: &md_db::schema::FieldDef) -> Value {
    let mut obj = json!({
        "name": f.name,
        "type": field_type_short(&f.field_type),
        "required": f.required,
    });
    if let Some(ref desc) = f.description {
        obj["description"] = Value::String(desc.clone());
    }
    if let Some(ref pat) = f.pattern {
        obj["pattern"] = Value::String(pat.clone());
    }
    if let Some(ref def) = f.default {
        obj["default"] = Value::String(def.clone());
    }
    if let md_db::schema::FieldType::Enum(ref vals) = f.field_type {
        obj["values"] = json!(vals);
    }
    obj
}

fn section_to_json(s: &md_db::schema::SectionDef) -> Value {
    let mut obj = json!({ "name": s.name, "required": s.required });
    if let Some(ref desc) = s.description {
        obj["description"] = Value::String(desc.clone());
    }
    if !s.children.is_empty() {
        let children: Vec<Value> = s.children.iter().map(|c| section_to_json(c)).collect();
        obj["children"] = json!(children);
    }
    obj
}

fn type_to_json(type_def: &md_db::schema::TypeDef) -> Value {
    let fields: Vec<Value> = type_def.fields.iter().map(|f| field_to_json(f)).collect();
    let sections: Vec<Value> = type_def.sections.iter().map(|s| section_to_json(s)).collect();
    json!({
        "name": type_def.name,
        "description": type_def.description,
        "folder": type_def.folder,
        "max_count": type_def.max_count,
        "fields": fields,
        "sections": sections,
    })
}

fn export_schema_json(schema: &Schema) -> Value {
    let types: Vec<Value> = schema.types.iter().map(|t| type_to_json(t)).collect();
    json!({ "types": types, "relations": relations_to_json(schema) })
}

fn relations_to_json(schema: &Schema) -> Value {
    let rels: Vec<Value> = schema
        .relations
        .iter()
        .map(|r| {
            json!({
                "name": r.name,
                "inverse": r.inverse,
                "cardinality": match r.cardinality {
                    md_db::schema::Cardinality::One => "one",
                    md_db::schema::Cardinality::Many => "many",
                },
                "description": r.description,
                "acyclic": r.acyclic,
            })
        })
        .collect();
    json!(rels)
}

fn parse_cell_spec(spec: &str) -> Result<(String, usize), String> {
    let parts: Vec<&str> = spec.splitn(2, ',').collect();
    if parts.len() != 2 {
        return Err(format!("invalid cell spec '{spec}', expected Column,Row"));
    }
    let col = parts[0].to_string();
    let row: usize = parts[1].parse().map_err(|e: std::num::ParseIntError| e.to_string())?;
    Ok((col, row))
}

// ── Main loop ───────────────────────────────────────────────────────────────

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = stdin.lock();
    let mut writer = stdout.lock();

    let mut initialized = false;

    loop {
        let mut line = String::new();
        let bytes_read = reader.read_line(&mut line)?;
        if bytes_read == 0 {
            break; // EOF
        }

        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let request: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                let resp = jsonrpc_error(&Value::Null, -32700, &format!("parse error: {e}"));
                writeln!(writer, "{}", resp)?;
                writer.flush()?;
                continue;
            }
        };

        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request
            .get("method")
            .and_then(|m| m.as_str())
            .unwrap_or("");
        let params = request.get("params").cloned().unwrap_or(json!({}));

        let response = match method {
            "initialize" => {
                initialized = true;
                jsonrpc_ok(
                    &id,
                    json!({
                        "protocolVersion": "2024-11-05",
                        "capabilities": {
                            "tools": { "listChanged": false }
                        },
                        "serverInfo": {
                            "name": "md-db",
                            "version": env!("CARGO_PKG_VERSION"),
                        }
                    }),
                )
            }
            "notifications/initialized" => {
                // Client acknowledgement — no response needed for notifications
                continue;
            }
            "tools/list" => {
                if !initialized {
                    jsonrpc_error(&id, -32600, "not initialized")
                } else {
                    jsonrpc_ok(&id, json!({ "tools": tool_list() }))
                }
            }
            "tools/call" => {
                if !initialized {
                    jsonrpc_error(&id, -32600, "not initialized")
                } else {
                    let tool_name = params
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("");
                    let tool_args = params.get("arguments").cloned().unwrap_or(json!({}));

                    match handle_tool_call(tool_name, &tool_args) {
                        Ok(result) => {
                            let text = serde_json::to_string_pretty(&result)
                                .unwrap_or_else(|_| result.to_string());
                            jsonrpc_ok(
                                &id,
                                json!({
                                    "content": text_content(&text),
                                    "isError": false,
                                }),
                            )
                        }
                        Err(e) => jsonrpc_ok(
                            &id,
                            json!({
                                "content": text_content(&e),
                                "isError": true,
                            }),
                        ),
                    }
                }
            }
            "ping" => jsonrpc_ok(&id, json!({})),
            _ => jsonrpc_error(&id, -32601, &format!("unknown method: {method}")),
        };

        writeln!(writer, "{}", response)?;
        writer.flush()?;
    }

    Ok(())
}
