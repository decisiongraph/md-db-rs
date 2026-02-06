use std::path::PathBuf;

use clap::Args;
use md_db::migrate;
use md_db::schema::Schema;

#[derive(Debug, Args)]
pub struct MigrateArgs {
    /// Directory containing documents to migrate
    pub dir: Option<PathBuf>,

    /// Path to the old (current) schema file
    #[arg(long, alias = "from")]
    pub old_schema: PathBuf,

    /// Path to the new (target) schema file
    #[arg(long, alias = "to")]
    pub new_schema: PathBuf,

    /// Show diff and plan without applying changes
    #[arg(long)]
    pub dry_run: bool,

    /// Output format: text, json (default: text)
    #[arg(long, default_value = "text")]
    pub format: String,
}

pub fn run(args: &MigrateArgs) -> Result<(), Box<dyn std::error::Error>> {
    let old_schema = Schema::from_file(&args.old_schema)?;
    let new_schema = Schema::from_file(&args.new_schema)?;

    let diff = migrate::diff_schemas(&old_schema, &new_schema);

    if diff.is_empty() {
        println!("Schemas are identical — no migration needed.");
        return Ok(());
    }

    let format = md_db::output::OutputFormat::from_str(&args.format)
        .unwrap_or(md_db::output::OutputFormat::Text);

    match format {
        md_db::output::OutputFormat::Json => {
            print_json(&diff, args)?;
        }
        _ => {
            print!("{diff}");
            if let Some(ref dir) = args.dir {
                let plan = migrate::compute_migration(&diff, dir);
                println!();
                print!("{plan}");
                if !args.dry_run && !plan.actions.is_empty() {
                    let result = migrate::apply_migration(&plan)?;
                    println!();
                    println!("{result}");
                }
            } else if !args.dry_run {
                eprintln!("hint: pass a directory to scan documents and compute a migration plan");
            }
        }
    }

    Ok(())
}

fn print_json(
    diff: &migrate::SchemaDiff,
    args: &MigrateArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut obj = serde_json::Map::new();

    // Diff section
    let mut diff_obj = serde_json::Map::new();
    diff_obj.insert(
        "added_types".into(),
        serde_json::Value::Array(
            diff.added_types
                .iter()
                .map(|s| serde_json::Value::String(s.clone()))
                .collect(),
        ),
    );
    diff_obj.insert(
        "removed_types".into(),
        serde_json::Value::Array(
            diff.removed_types
                .iter()
                .map(|s| serde_json::Value::String(s.clone()))
                .collect(),
        ),
    );

    let type_changes: Vec<serde_json::Value> = diff
        .type_changes
        .iter()
        .map(|tc| {
            serde_json::json!({
                "type": tc.type_name,
                "added_fields": tc.added_fields.iter().map(|f| &f.name).collect::<Vec<_>>(),
                "removed_fields": tc.removed_fields.iter().map(|f| &f.name).collect::<Vec<_>>(),
                "changed_fields": tc.changed_fields.iter().map(|c| {
                    serde_json::json!({
                        "name": c.name,
                        "removed_enum_values": c.removed_enum_values,
                        "added_enum_values": c.added_enum_values,
                    })
                }).collect::<Vec<_>>(),
                "added_sections": tc.added_sections,
                "removed_sections": tc.removed_sections,
            })
        })
        .collect();
    diff_obj.insert("type_changes".into(), serde_json::Value::Array(type_changes));
    obj.insert("diff".into(), serde_json::Value::Object(diff_obj));

    // Plan section (if dir provided)
    if let Some(ref dir) = args.dir {
        let plan = migrate::compute_migration(diff, dir);
        let actions: Vec<serde_json::Value> = plan
            .actions
            .iter()
            .map(|a| {
                let kind = match &a.kind {
                    migrate::ActionKind::AddField {
                        type_name,
                        field_name,
                        default_value,
                    } => serde_json::json!({
                        "action": "add_field",
                        "type": type_name,
                        "field": field_name,
                        "default": default_value,
                    }),
                    migrate::ActionKind::RemoveField {
                        type_name,
                        field_name,
                    } => serde_json::json!({
                        "action": "remove_field",
                        "type": type_name,
                        "field": field_name,
                    }),
                    migrate::ActionKind::RemovedEnumValue {
                        type_name,
                        field_name,
                        value,
                    } => serde_json::json!({
                        "action": "removed_enum_value",
                        "type": type_name,
                        "field": field_name,
                        "value": value,
                    }),
                    migrate::ActionKind::AddSection {
                        type_name,
                        section_name,
                    } => serde_json::json!({
                        "action": "add_section",
                        "type": type_name,
                        "section": section_name,
                    }),
                };
                let docs: Vec<String> = a
                    .affected_docs
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect();
                serde_json::json!({
                    "kind": kind,
                    "affected_docs": docs,
                    "count": a.affected_docs.len(),
                })
            })
            .collect();
        obj.insert("plan".into(), serde_json::Value::Array(actions));
        obj.insert("dry_run".into(), serde_json::Value::Bool(args.dry_run));
    }

    println!("{}", serde_json::to_string_pretty(&serde_json::Value::Object(obj))?);
    Ok(())
}
