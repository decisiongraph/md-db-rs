use std::collections::HashSet;
use std::path::PathBuf;

use clap::Args;
use md_db::document::Document;
use md_db::graph::{path_to_id, DocGraph};
use md_db::schema::{FieldType, Schema};

#[derive(Debug, Args)]
pub struct RenameArgs {
    /// Source file to rename
    pub file: PathBuf,

    /// New document ID (e.g. ADR-010)
    pub new_id: String,

    /// Path to KDL schema file
    #[arg(long)]
    pub schema: PathBuf,

    /// Directory to scan for references
    #[arg(long)]
    pub dir: PathBuf,

    /// Dry run -- show changes without writing
    #[arg(long)]
    pub dry_run: bool,
}

pub fn run(args: &RenameArgs) -> Result<(), Box<dyn std::error::Error>> {
    let schema = Schema::from_file(&args.schema)?;
    let old_id = path_to_id(&args.file);
    let new_id = args.new_id.to_uppercase();

    if old_id == new_id {
        return Err(format!("old ID and new ID are the same: {old_id}").into());
    }

    // Compute new filename: lowercase new_id + preserve slug if any + .md
    let new_filename = compute_new_filename(&args.file, &old_id, &new_id);
    let new_path = args
        .file
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join(&new_filename);

    if new_path.exists() && new_path != args.file {
        return Err(format!("target file already exists: {}", new_path.display()).into());
    }

    // Build graph to find all docs referencing old_id
    let graph = DocGraph::build(&args.dir, &schema)?;
    let backlinks = graph.refs_to(&old_id);

    // Collect unique referencing doc IDs (skip self)
    let referencing_ids: HashSet<&str> = backlinks
        .iter()
        .map(|e| e.from.as_str())
        .filter(|id| *id != old_id)
        .collect();

    // Collect all field names that can hold refs (relation fields + type ref/ref[] fields)
    let mut ref_field_names: HashSet<String> = HashSet::new();
    for name in schema.all_relation_field_names() {
        ref_field_names.insert(name.to_string());
    }
    for type_def in &schema.types {
        for field in &type_def.fields {
            if field.field_type == FieldType::Ref || field.field_type == FieldType::RefArray {
                ref_field_names.insert(field.name.clone());
            }
        }
    }

    let mut updated_files = Vec::new();

    // Update each referencing document
    for ref_id in &referencing_ids {
        let node = match graph.nodes.get(*ref_id) {
            Some(n) => n,
            None => continue,
        };

        let mut doc = Document::from_file(&node.path)?;
        let fm = match doc.frontmatter.as_mut() {
            Some(fm) => fm,
            None => continue,
        };

        let mut changed = false;

        {
            let data = fm.data_mut();
            for field_name in &ref_field_names {
                if let Some(val) = data.get_mut(field_name) {
                    if replace_ref_in_value(val, &old_id, &new_id) {
                        changed = true;
                    }
                }
            }
        }

        if changed {
            // Rebuild raw from updated frontmatter -- rebuild_raw is private,
            // so we re-set a sentinel field to trigger it, then use set_field
            // Actually, we can use set_field with the already-mutated values
            // to force a rebuild. Simplest: just re-serialize manually.
            let fm_ref = doc.frontmatter.as_ref().unwrap();
            let yaml = fm_ref.to_yaml_string();
            let mut raw = String::new();
            raw.push_str("---\n");
            raw.push_str(&yaml);
            raw.push_str("---\n");
            raw.push_str(&doc.body);
            doc.raw = raw;

            if args.dry_run {
                eprintln!("  would update: {} ({})", node.path.display(), ref_id);
            } else {
                doc.save()?;
                eprintln!("  updated: {} ({})", node.path.display(), ref_id);
            }
            updated_files.push(node.path.clone());
        }
    }

    // Rename the source file
    if args.dry_run {
        eprintln!(
            "  would rename: {} -> {}",
            args.file.display(),
            new_path.display()
        );
    } else {
        std::fs::rename(&args.file, &new_path)?;
        eprintln!("  renamed: {} -> {}", args.file.display(), new_path.display());
    }

    // Summary
    eprintln!(
        "rename {old_id} -> {new_id}: {} file(s) updated, 1 file renamed",
        updated_files.len()
    );

    Ok(())
}

/// Compute the new filename preserving any slug suffix.
///
/// Example: `adr-001-use-postgresql.md` with new_id=`ADR-010`
///   -> `adr-010-use-postgresql.md`
///
/// Example: `adr-001.md` with new_id=`ADR-010` -> `adr-010.md`
fn compute_new_filename(old_path: &std::path::Path, old_id: &str, new_id: &str) -> String {
    let stem = old_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");

    // old_id is uppercase (e.g. "ADR-001"), stem may be lowercase "adr-001-use-postgresql"
    // Normalise for comparison: uppercase stem, replace _ with -
    let stem_upper = stem.to_uppercase().replace('_', "-");

    let slug = if stem_upper.starts_with(old_id) {
        // Everything after the ID prefix in the original stem
        &stem[old_id.len()..]
    } else {
        ""
    };

    format!("{}{slug}.md", new_id.to_lowercase())
}

/// Replace occurrences of old_id with new_id in a YAML value (case-insensitive match).
/// Returns true if any replacement was made.
fn replace_ref_in_value(val: &mut serde_yaml::Value, old_id: &str, new_id: &str) -> bool {
    match val {
        serde_yaml::Value::String(s) => {
            if s.to_uppercase() == old_id {
                *s = new_id.to_string();
                true
            } else {
                false
            }
        }
        serde_yaml::Value::Sequence(seq) => {
            let mut changed = false;
            for item in seq.iter_mut() {
                if replace_ref_in_value(item, old_id, new_id) {
                    changed = true;
                }
            }
            changed
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_compute_new_filename_simple() {
        let path = Path::new("docs/adr-001.md");
        assert_eq!(compute_new_filename(path, "ADR-001", "ADR-010"), "adr-010.md");
    }

    #[test]
    fn test_compute_new_filename_with_slug() {
        let path = Path::new("docs/adr-001-use-postgresql.md");
        assert_eq!(
            compute_new_filename(path, "ADR-001", "ADR-010"),
            "adr-010-use-postgresql.md"
        );
    }

    #[test]
    fn test_compute_new_filename_underscore() {
        let path = Path::new("inc_002.md");
        assert_eq!(compute_new_filename(path, "INC-002", "INC-010"), "inc-010.md");
    }

    #[test]
    fn test_replace_ref_string() {
        let mut val = serde_yaml::Value::String("ADR-001".into());
        assert!(replace_ref_in_value(&mut val, "ADR-001", "ADR-010"));
        assert_eq!(val, serde_yaml::Value::String("ADR-010".into()));
    }

    #[test]
    fn test_replace_ref_string_case_insensitive() {
        let mut val = serde_yaml::Value::String("adr-001".into());
        assert!(replace_ref_in_value(&mut val, "ADR-001", "ADR-010"));
        assert_eq!(val, serde_yaml::Value::String("ADR-010".into()));
    }

    #[test]
    fn test_replace_ref_array() {
        let mut val = serde_yaml::Value::Sequence(vec![
            serde_yaml::Value::String("ADR-001".into()),
            serde_yaml::Value::String("ADR-002".into()),
        ]);
        assert!(replace_ref_in_value(&mut val, "ADR-001", "ADR-010"));
        let expected = serde_yaml::Value::Sequence(vec![
            serde_yaml::Value::String("ADR-010".into()),
            serde_yaml::Value::String("ADR-002".into()),
        ]);
        assert_eq!(val, expected);
    }

    #[test]
    fn test_replace_ref_no_match() {
        let mut val = serde_yaml::Value::String("ADR-999".into());
        assert!(!replace_ref_in_value(&mut val, "ADR-001", "ADR-010"));
        assert_eq!(val, serde_yaml::Value::String("ADR-999".into()));
    }
}
