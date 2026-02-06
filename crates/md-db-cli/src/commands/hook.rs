use clap::Args;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Args)]
pub struct HookArgs {
    /// Action: install or uninstall
    pub action: String,

    /// Git repo directory (default: current directory)
    #[arg(long, default_value = ".")]
    pub dir: PathBuf,

    /// Schema file path relative to repo root
    #[arg(long, default_value = "schema.kdl")]
    pub schema: String,
}

const HOOK_TEMPLATE: &str = r#"#!/usr/bin/env bash
# md-db pre-commit hook — validates changed markdown files
set -euo pipefail

changed=$(git diff --cached --name-only --diff-filter=ACM -- '*.md')
if [ -n "$changed" ]; then
    echo "$changed" | md-db validate --stdin-list --schema {SCHEMA}
fi
"#;

pub fn run(args: &HookArgs) -> Result<(), Box<dyn std::error::Error>> {
    match args.action.as_str() {
        "install" => install(args),
        "uninstall" => uninstall(args),
        _ => Err(format!("unknown action: {} (expected: install, uninstall)", args.action).into()),
    }
}

fn install(args: &HookArgs) -> Result<(), Box<dyn std::error::Error>> {
    let hooks_dir = args.dir.join(".git/hooks");
    if !hooks_dir.exists() {
        return Err("not a git repository (no .git/hooks directory)".into());
    }

    let hook_path = hooks_dir.join("pre-commit");
    if hook_path.exists() {
        return Err("pre-commit hook already exists — remove it first or use 'uninstall'".into());
    }

    let hook_content = HOOK_TEMPLATE.replace("{SCHEMA}", &args.schema);
    fs::write(&hook_path, hook_content)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755))?;
    }

    println!("Installed pre-commit hook at {}", hook_path.display());
    Ok(())
}

fn uninstall(args: &HookArgs) -> Result<(), Box<dyn std::error::Error>> {
    let hook_path = args.dir.join(".git/hooks/pre-commit");
    if hook_path.exists() {
        fs::remove_file(&hook_path)?;
        println!("Removed pre-commit hook");
    } else {
        println!("No pre-commit hook found");
    }
    Ok(())
}
