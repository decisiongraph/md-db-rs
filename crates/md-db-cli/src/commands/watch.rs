use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use clap::Args;
use md_db::document::Document;
use md_db::output::OutputFormat;
use md_db::schema::Schema;
use md_db::users::UserConfig;
use md_db::validation::{self, FileResult, Severity, ValidationResult};
use notify::{EventKind, RecursiveMode, Watcher};

#[derive(Debug, Args)]
pub struct WatchArgs {
    /// Directory to watch
    pub dir: PathBuf,

    /// Path to KDL schema file
    #[arg(long)]
    pub schema: PathBuf,

    /// Path to user/team config YAML file
    #[arg(long)]
    pub users: Option<PathBuf>,

    /// Output format: text, json, compact
    #[arg(long, default_value = "text")]
    pub format: String,

    /// Debounce interval in milliseconds
    #[arg(long, default_value = "300")]
    pub debounce: u64,
}

pub fn run(args: &WatchArgs) -> Result<(), Box<dyn std::error::Error>> {
    let schema = Schema::from_file(&args.schema)?;
    let user_config = match &args.users {
        Some(path) => Some(UserConfig::from_file(path)?),
        None => None,
    };
    let format = OutputFormat::from_str(&args.format).unwrap_or(OutputFormat::Text);
    let debounce_dur = Duration::from_millis(args.debounce);

    // Initial full validation
    eprintln!("Watching {} for changes...", args.dir.display());
    let result = validation::validate_directory(&args.dir, &schema, None, user_config.as_ref())?;
    print_result(&result, format, None);

    // Set up file watcher
    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(event) = res {
            let _ = tx.send(event);
        }
    })?;

    watcher.watch(&args.dir, RecursiveMode::Recursive)?;

    // Also watch schema file for changes
    let schema_path = args.schema.canonicalize().unwrap_or_else(|_| args.schema.clone());
    if let Some(schema_parent) = schema_path.parent() {
        let _ = watcher.watch(schema_parent, RecursiveMode::NonRecursive);
    }

    // Also watch users file if specified
    let users_path = args.users.as_ref().and_then(|p| p.canonicalize().ok());
    if let Some(ref up) = users_path {
        if let Some(parent) = up.parent() {
            let _ = watcher.watch(parent, RecursiveMode::NonRecursive);
        }
    }


    loop {
        // Collect events with debouncing
        let event = rx.recv()?;
        let mut changed_paths: HashSet<PathBuf> = collect_paths(&event);

        // Drain any additional events within debounce window
        loop {
            match rx.recv_timeout(debounce_dur) {
                Ok(ev) => {
                    changed_paths.extend(collect_paths(&ev));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("file watcher disconnected".into())
                }
            }
        }

        // Filter to relevant files
        let schema_changed = changed_paths.iter().any(|p| {
            p.canonicalize().unwrap_or_else(|_| p.clone()) == schema_path
        });
        let users_changed = users_path.as_ref().map_or(false, |up| {
            changed_paths.iter().any(|p| {
                p.canonicalize().unwrap_or_else(|_| p.clone()) == *up
            })
        });

        // Reload schema/users if changed
        let current_schema = if schema_changed {
            match Schema::from_file(&args.schema) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("[{}] schema reload error: {e}", timestamp());
                    continue;
                }
            }
        } else {
            schema.clone()
        };
        let current_users = if users_changed {
            match &args.users {
                Some(path) => match UserConfig::from_file(path) {
                    Ok(u) => Some(u),
                    Err(e) => {
                        eprintln!("[{}] users reload error: {e}", timestamp());
                        user_config.clone()
                    }
                },
                None => None,
            }
        } else {
            user_config.clone()
        };

        if schema_changed || users_changed {
            // Full re-validation
            match validation::validate_directory(
                &args.dir,
                &current_schema,
                None,
                current_users.as_ref(),
            ) {
                Ok(result) => print_result(&result, format, None),
                Err(e) => eprintln!("[{}] validation error: {e}", timestamp()),
            }
        } else {
            // Incremental: validate only changed .md files
            let md_files: Vec<PathBuf> = changed_paths
                .into_iter()
                .filter(|p| p.extension().map_or(false, |ext| ext == "md") && p.is_file())
                .collect();

            if md_files.is_empty() {
                continue;
            }

            // Build known files/IDs from the whole directory for cross-ref validation
            let all_files =
                md_db::discovery::discover_files(&args.dir, None, &[]).unwrap_or_default();
            let known_files: HashSet<PathBuf> = all_files
                .iter()
                .map(|p| p.canonicalize().unwrap_or_else(|_| p.clone()))
                .collect();
            let known_ids: HashSet<String> =
                all_files.iter().map(|p| md_db::graph::path_to_id(p)).collect();

            let mut file_results = Vec::new();
            for path in &md_files {
                match Document::from_file(path) {
                    Ok(doc) => {
                        // Skip files without frontmatter type
                        if doc.frontmatter.is_none() {
                            continue;
                        }
                        if let Some(ref fm) = doc.frontmatter {
                            if fm.get("type").is_none() {
                                continue;
                            }
                        }
                        file_results.push(validation::validate_document(
                            &doc,
                            &current_schema,
                            &known_files,
                            &known_ids,
                            current_users.as_ref(),
                        ));
                    }
                    Err(e) => {
                        file_results.push(FileResult {
                            path: path.display().to_string(),
                            diagnostics: vec![validation::Diagnostic {
                                severity: Severity::Error,
                                code: "E000".into(),
                                message: format!("failed to parse: {e}"),
                                location: "file".into(),
                                hint: None,
                            }],
                        });
                    }
                }
            }

            if !file_results.is_empty() {
                let result = ValidationResult { file_results };
                let changed_display: Vec<String> =
                    md_files.iter().map(|p| p.display().to_string()).collect();
                print_result(&result, format, Some(&changed_display));
            }
        }
    }
}

fn collect_paths(event: &notify::Event) -> HashSet<PathBuf> {
    match event.kind {
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => {
            event.paths.iter().cloned().collect()
        }
        _ => HashSet::new(),
    }
}

fn timestamp() -> String {
    let now = std::time::SystemTime::now();
    let since_midnight = now
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        % 86400;
    let h = since_midnight / 3600;
    let m = (since_midnight % 3600) / 60;
    let s = since_midnight % 60;
    format!("{h:02}:{m:02}:{s:02}")
}

fn print_result(result: &ValidationResult, format: OutputFormat, changed: Option<&[String]>) {
    match format {
        OutputFormat::Json => {
            let files: Vec<serde_json::Value> = result
                .file_results
                .iter()
                .map(|f| {
                    let diags: Vec<serde_json::Value> = f
                        .diagnostics
                        .iter()
                        .map(|d| {
                            serde_json::json!({
                                "severity": d.severity.to_string(),
                                "code": d.code,
                                "message": d.message,
                                "location": d.location,
                                "hint": d.hint,
                            })
                        })
                        .collect();
                    serde_json::json!({
                        "path": f.path,
                        "diagnostics": diags,
                        "errors": f.errors(),
                        "warnings": f.warnings(),
                    })
                })
                .collect();
            let json = serde_json::json!({
                "timestamp": timestamp(),
                "files": files,
                "errors": result.total_errors(),
                "warnings": result.total_warnings(),
                "ok": result.is_ok(),
            });
            println!("{}", serde_json::to_string(&json).unwrap_or_default());
        }
        OutputFormat::Compact => {
            if let Some(paths) = changed {
                for path in paths {
                    let fr = result.file_results.iter().find(|f| f.path == *path);
                    match fr {
                        Some(f) if f.diagnostics.is_empty() => {
                            println!("[{}] {} — 0 errors, 0 warnings ✓", timestamp(), path);
                        }
                        Some(f) => {
                            println!(
                                "[{}] {} — {} error(s), {} warning(s)",
                                timestamp(),
                                path,
                                f.errors(),
                                f.warnings()
                            );
                            for d in &f.diagnostics {
                                println!("  {}", d.to_compact());
                            }
                        }
                        None => {
                            println!("[{}] {} — 0 errors, 0 warnings ✓", timestamp(), path);
                        }
                    }
                }
            } else {
                print!("{}", result.to_compact_report());
                println!(
                    "result: {} error(s), {} warning(s)",
                    result.total_errors(),
                    result.total_warnings()
                );
            }
        }
        _ => {
            // Text format: clear screen + show current state
            if changed.is_some() {
                // Clear screen for incremental updates
                print!("\x1B[2J\x1B[H");
            }
            if let Some(paths) = changed {
                for path in paths {
                    let fr = result.file_results.iter().find(|f| f.path == *path);
                    match fr {
                        Some(f) if f.diagnostics.is_empty() => {
                            eprintln!(
                                "[{}] {} changed — 0 errors, 0 warnings ✓",
                                timestamp(),
                                path
                            );
                        }
                        Some(f) => {
                            eprintln!(
                                "[{}] {} changed — {} error(s)",
                                timestamp(),
                                path,
                                f.errors()
                            );
                            for d in &f.diagnostics { println!("{d}"); }
                        }
                        None => {
                            eprintln!(
                                "[{}] {} changed — 0 errors, 0 warnings ✓",
                                timestamp(),
                                path
                            );
                        }
                    }
                }
            }
            if result.total_errors() > 0 || result.total_warnings() > 0 {
                print!("{}", result.to_report());
            } else {
                eprintln!(
                    "[{}] all files valid ✓ ({} error(s), {} warning(s))",
                    timestamp(),
                    result.total_errors(),
                    result.total_warnings()
                );
            }
        }
    }
}
