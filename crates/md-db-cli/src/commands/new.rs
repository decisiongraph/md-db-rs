use std::path::PathBuf;

use clap::Args;
use md_db::error::Error;
use md_db::graph::DocGraph;
use md_db::schema::Schema;
use md_db::template;

#[derive(Debug, Args)]
pub struct NewArgs {
    /// Document type name from the schema
    #[arg(long = "type")]
    pub doc_type: String,

    /// Path to the KDL schema file
    #[arg(long)]
    pub schema: PathBuf,

    /// Output file path (prints to stdout if omitted; use --auto-id to generate path automatically)
    #[arg(long)]
    pub output: Option<PathBuf>,

    /// Directory to scan for auto-ID generation (next available ID)
    #[arg(long)]
    pub dir: Option<PathBuf>,

    /// Pre-fill field values (repeatable): key=value
    #[arg(long = "field")]
    pub fields: Vec<String>,

    /// Expand all template variables and date placeholders to real values
    #[arg(long)]
    pub fill: bool,

    /// Auto-generate output path using next ID + type folder (requires --dir)
    #[arg(long)]
    pub auto_id: bool,
}

pub fn run(args: &NewArgs) -> Result<(), Box<dyn std::error::Error>> {
    let schema = Schema::from_file(&args.schema)?;

    let type_def = schema
        .get_type(&args.doc_type)
        .ok_or(Error::TypeNotFound(args.doc_type.clone()))?;

    let fields: Vec<(String, String)> = args
        .fields
        .iter()
        .map(|s| parse_field_arg(s))
        .collect::<Result<_, _>>()?;

    // Auto-ID: scan dir, compute next ID, generate output path
    let output_path = if args.auto_id {
        let dir = args.dir.as_ref().ok_or("--auto-id requires --dir")?;
        let graph = DocGraph::build(dir, &schema)?;
        let next_id = graph.next_id(&args.doc_type);
        let folder = type_def.folder.as_deref().unwrap_or(".");
        let filename = format!("{}.md", next_id.to_lowercase());
        let path = PathBuf::from(dir).join(folder).join(&filename);
        eprintln!("auto-id: {next_id} → {}", path.display());
        Some(path)
    } else if let Some(ref dir) = args.dir {
        // --dir without --auto-id: just print next available ID
        let graph = DocGraph::build(dir, &schema)?;
        let next_id = graph.next_id(&args.doc_type);
        eprintln!("next-id: {next_id}");
        args.output.clone()
    } else {
        args.output.clone()
    };

    let content = template::generate_document_opts(type_def, &schema, &fields, args.fill);

    if let Some(ref path) = output_path {
        // Create parent directories if needed
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)?;
            }
        }
        std::fs::write(path, &content)?;
        eprintln!("wrote {}", path.display());
    } else {
        print!("{content}");
        if let Some(ref folder) = type_def.folder {
            eprintln!("hint: default folder for type \"{}\" is \"{folder}\"", type_def.name);
        }
    }

    Ok(())
}

fn parse_field_arg(s: &str) -> Result<(String, String), Box<dyn std::error::Error>> {
    let (key, value) = s
        .split_once('=')
        .ok_or_else(|| format!("invalid --field format '{}', expected key=value", s))?;
    Ok((key.to_string(), value.to_string()))
}
