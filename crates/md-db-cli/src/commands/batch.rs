use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use clap::Args;
use md_db::discovery::{self, Filter};
use md_db::document::Document;

#[derive(Debug, Args)]
pub struct BatchArgs {
    /// Directory to scan
    pub dir: PathBuf,

    /// Field filters (key=value)
    #[arg(long = "field", num_args = 1)]
    pub fields: Vec<String>,

    /// NOT equal filters (key=value or key!=value)
    #[arg(long = "not-field", num_args = 1)]
    pub not_fields: Vec<String>,

    /// Has-field filters
    #[arg(long = "has-field", num_args = 1)]
    pub has_fields: Vec<String>,

    /// Contains filters (key~=value or key=value)
    #[arg(long = "contains", num_args = 1)]
    pub contains: Vec<String>,

    /// Set field values (key=value) — applied to all matching docs
    #[arg(long = "set", num_args = 1, required = true)]
    pub set_fields: Vec<String>,

    /// Dry run — show what would change without writing
    #[arg(long)]
    pub dry_run: bool,

    /// Skip confirmation prompt
    #[arg(long)]
    pub yes: bool,

    /// Glob pattern for filenames (default: "*.md")
    #[arg(long)]
    pub pattern: Option<String>,
}

pub fn run(args: &BatchArgs) -> Result<(), Box<dyn std::error::Error>> {
    // Require at least one filter for safety
    if args.fields.is_empty()
        && args.not_fields.is_empty()
        && args.has_fields.is_empty()
        && args.contains.is_empty()
        && args.pattern.is_none()
    {
        return Err(
            "at least one filter is required (--field, --not-field, --has-field, --contains, or --pattern)"
                .into(),
        );
    }

    // Parse --set pairs upfront so we fail fast on bad syntax
    let set_pairs: Vec<(&str, &str)> = args
        .set_fields
        .iter()
        .map(|s| {
            s.split_once('=')
                .ok_or_else(|| format!("invalid --set format '{}', expected key=value", s))
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;

    // Build filters (same logic as list.rs)
    let mut filters = Vec::new();
    for f in &args.fields {
        if let Some((key, value)) = f.split_once('=') {
            filters.push(Filter::FieldEquals {
                key: key.to_string(),
                value: value.to_string(),
            });
        }
    }
    for f in &args.not_fields {
        if let Some((key, value)) = f.split_once("!=") {
            filters.push(Filter::FieldNotEquals {
                key: key.to_string(),
                value: value.to_string(),
            });
        } else if let Some((key, value)) = f.split_once('=') {
            filters.push(Filter::FieldNotEquals {
                key: key.to_string(),
                value: value.to_string(),
            });
        }
    }
    for f in &args.contains {
        if let Some((key, value)) = f.split_once("~=") {
            filters.push(Filter::FieldContains {
                key: key.to_string(),
                value: value.to_string(),
            });
        } else if let Some((key, value)) = f.split_once('=') {
            filters.push(Filter::FieldContains {
                key: key.to_string(),
                value: value.to_string(),
            });
        }
    }
    for f in &args.has_fields {
        filters.push(Filter::HasField(f.clone()));
    }

    let pattern = args.pattern.as_deref();
    let files = discovery::discover_files(&args.dir, pattern, &filters)?;

    if files.is_empty() {
        println!("0 documents match. Nothing to do.");
        return Ok(());
    }

    // Confirmation prompt (skip for --yes or --dry-run)
    if !args.yes && !args.dry_run {
        print!(
            "{} document(s) match. Apply changes? [y/N] ",
            files.len()
        );
        io::stdout().flush()?;
        let mut answer = String::new();
        io::stdin().lock().read_line(&mut answer)?;
        if !answer.trim().eq_ignore_ascii_case("y") {
            println!("Aborted.");
            return Ok(());
        }
    }

    let mut changed = 0usize;
    for path in &files {
        if args.dry_run {
            println!("[dry-run] {}", path.display());
            changed += 1;
            continue;
        }

        let mut doc = Document::from_file(path)?;
        for &(key, value) in &set_pairs {
            doc.set_field_from_str(key, value);
        }
        doc.save()?;
        println!("updated {}", path.display());
        changed += 1;
    }

    if args.dry_run {
        println!(
            "\n{} document(s) would be updated (dry run).",
            changed
        );
    } else {
        println!("\n{} document(s) updated.", changed);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_doc(dir: &std::path::Path, name: &str, content: &str) {
        fs::write(dir.join(name), content).unwrap();
    }

    #[test]
    fn test_batch_dry_run() {
        let dir = tempfile::tempdir().unwrap();
        write_doc(
            dir.path(),
            "a.md",
            "---\ntype: adr\nstatus: proposed\n---\n# A\n",
        );
        write_doc(
            dir.path(),
            "b.md",
            "---\ntype: adr\nstatus: accepted\n---\n# B\n",
        );
        write_doc(
            dir.path(),
            "c.md",
            "---\ntype: inc\nstatus: proposed\n---\n# C\n",
        );

        let args = BatchArgs {
            dir: dir.path().to_path_buf(),
            fields: vec!["type=adr".to_string()],
            not_fields: vec![],
            has_fields: vec![],
            contains: vec![],
            set_fields: vec!["status=needs-review".to_string()],
            dry_run: true,
            yes: false,
            pattern: None,
        };

        run(&args).unwrap();

        // Verify files were NOT modified (dry run)
        let a = fs::read_to_string(dir.path().join("a.md")).unwrap();
        assert!(a.contains("status: proposed"));
        let b = fs::read_to_string(dir.path().join("b.md")).unwrap();
        assert!(b.contains("status: accepted"));
    }

    #[test]
    fn test_batch_apply() {
        let dir = tempfile::tempdir().unwrap();
        write_doc(
            dir.path(),
            "a.md",
            "---\ntype: adr\nstatus: proposed\n---\n# A\n",
        );
        write_doc(
            dir.path(),
            "b.md",
            "---\ntype: adr\nstatus: accepted\n---\n# B\n",
        );
        write_doc(
            dir.path(),
            "c.md",
            "---\ntype: inc\nstatus: proposed\n---\n# C\n",
        );

        let args = BatchArgs {
            dir: dir.path().to_path_buf(),
            fields: vec!["type=adr".to_string()],
            not_fields: vec![],
            has_fields: vec![],
            contains: vec![],
            set_fields: vec!["status=needs-review".to_string()],
            dry_run: false,
            yes: true,
            pattern: None,
        };

        run(&args).unwrap();

        // a.md and b.md should be updated, c.md untouched
        let a = fs::read_to_string(dir.path().join("a.md")).unwrap();
        assert!(a.contains("needs-review"), "a.md should be updated");
        let b = fs::read_to_string(dir.path().join("b.md")).unwrap();
        assert!(b.contains("needs-review"), "b.md should be updated");
        let c = fs::read_to_string(dir.path().join("c.md")).unwrap();
        assert!(
            c.contains("status: proposed"),
            "c.md should be untouched"
        );
    }

    #[test]
    fn test_batch_requires_filter() {
        let dir = tempfile::tempdir().unwrap();
        let args = BatchArgs {
            dir: dir.path().to_path_buf(),
            fields: vec![],
            not_fields: vec![],
            has_fields: vec![],
            contains: vec![],
            set_fields: vec!["status=x".to_string()],
            dry_run: false,
            yes: true,
            pattern: None,
        };

        let result = run(&args);
        assert!(result.is_err());
    }
}
