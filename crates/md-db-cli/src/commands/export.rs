use std::path::PathBuf;

use clap::Args;
use md_db::export;
use md_db::schema::Schema;

#[derive(Debug, Args)]
pub struct ExportArgs {
    /// Directory containing markdown files
    pub dir: PathBuf,

    /// Path to KDL schema file (enables backlinks)
    #[arg(long)]
    pub schema: Option<PathBuf>,

    /// Output directory for generated site
    #[arg(long, default_value = "site")]
    pub output: PathBuf,

    /// Output format (only "html" supported currently)
    #[arg(long, default_value = "html")]
    pub format: String,
}

pub fn run(args: &ExportArgs) -> Result<(), Box<dyn std::error::Error>> {
    if args.format != "html" {
        return Err(format!("unsupported format \"{}\", only html is supported", args.format).into());
    }

    let schema = match &args.schema {
        Some(path) => Some(Schema::from_file(path)?),
        None => None,
    };

    let count = export::export_site(&args.dir, schema.as_ref(), &args.output)?;

    eprintln!("exported {count} documents to {}", args.output.display());

    Ok(())
}
