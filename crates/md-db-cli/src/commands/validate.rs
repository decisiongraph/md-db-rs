use std::path::PathBuf;

use clap::Args;
use md_db::schema::Schema;
use md_db::users::UserConfig;
use md_db::validation;

#[derive(Debug, Args)]
pub struct ValidateArgs {
    /// Directory or file to validate (omit when using --stdin)
    pub dir: Option<PathBuf>,

    /// Path to KDL schema file
    #[arg(long)]
    pub schema: PathBuf,

    /// Read document from stdin instead of file
    #[arg(long)]
    pub stdin: bool,

    /// Accept newline-separated file paths from stdin
    #[arg(long)]
    pub stdin_list: bool,

    /// Path to user/team config YAML file
    #[arg(long)]
    pub users: Option<PathBuf>,

    /// Glob pattern for filenames (default: "*.md")
    #[arg(long)]
    pub pattern: Option<String>,

    /// Output format: text, json, compact, auto (auto=json when piped)
    #[arg(long, default_value = "auto")]
    pub format: String,
}

pub fn run(args: &ValidateArgs) -> Result<(), Box<dyn std::error::Error>> {
    let schema = Schema::from_file(&args.schema)?;
    let user_config = match &args.users {
        Some(path) => Some(UserConfig::from_file(path)?),
        None => None,
    };

    let result = if args.stdin {
        let mut content = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut content)?;
        let doc = md_db::document::Document::from_str(&content)?;
        let fr = validation::validate_document(
            &doc,
            &schema,
            &std::collections::HashSet::new(),
            &std::collections::HashSet::new(),
            user_config.as_ref(),
        );
        validation::ValidationResult {
            file_results: vec![fr],
        }
    } else if args.stdin_list {
        let mut input = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut input)?;
        let paths: Vec<PathBuf> = input
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty() && l.ends_with(".md"))
            .map(PathBuf::from)
            .filter(|p| p.exists())
            .collect();

        let known_files: std::collections::HashSet<PathBuf> = paths
            .iter()
            .filter_map(|p| p.canonicalize().ok())
            .collect();
        let known_ids: std::collections::HashSet<String> = paths
            .iter()
            .map(|p| md_db::graph::path_to_id(p))
            .collect();

        let mut file_results = Vec::new();
        for path in &paths {
            let doc = match md_db::document::Document::from_file(path) {
                Ok(d) => d,
                Err(e) => {
                    file_results.push(validation::FileResult {
                        path: path.display().to_string(),
                        diagnostics: vec![validation::Diagnostic {
                            severity: validation::Severity::Error,
                            code: "E000".into(),
                            message: format!("failed to parse: {e}"),
                            location: "file".into(),
                            hint: None,
                        }],
                    });
                    continue;
                }
            };
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
                &schema,
                &known_files,
                &known_ids,
                user_config.as_ref(),
            ));
        }
        validation::ValidationResult { file_results }
    } else {
        let dir = args
            .dir
            .as_ref()
            .ok_or("directory argument required when not using --stdin or --stdin-list")?;
        let pattern = args.pattern.as_deref();
        validation::validate_directory(dir, &schema, pattern, user_config.as_ref())?
    };

    let format = md_db::output::OutputFormat::from_str(&args.format)
        .unwrap_or(md_db::output::OutputFormat::Text);

    match format {
        md_db::output::OutputFormat::Json => {
            let json = result_to_json(&result);
            println!("{}", serde_json::to_string_pretty(&json)?);
        }
        md_db::output::OutputFormat::Compact => {
            print!("{}", result.to_compact_report());
        }
        _ => {
            print!("{}", result.to_report());
        }
    }

    if result.is_ok() {
        Ok(())
    } else {
        std::process::exit(1);
    }
}

fn result_to_json(result: &validation::ValidationResult) -> serde_json::Value {
    let files: Vec<serde_json::Value> = result
        .file_results
        .iter()
        .filter(|f| !f.diagnostics.is_empty())
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
            })
        })
        .collect();

    serde_json::json!({
        "files": files,
        "errors": result.total_errors(),
        "warnings": result.total_warnings(),
        "ok": result.is_ok(),
    })
}
