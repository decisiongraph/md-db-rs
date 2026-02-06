use std::path::PathBuf;

use clap::Args;
use md_db::schema::Schema;
use md_db::sync;

#[derive(Debug, Args)]
pub struct SyncArgs {
    /// Directory containing markdown files
    pub dir: PathBuf,

    /// Path to KDL schema file
    #[arg(long)]
    pub schema: PathBuf,

    /// Show what would change without writing files
    #[arg(long)]
    pub dry_run: bool,

    /// Output format: text, json
    #[arg(long, default_value = "text")]
    pub format: String,
}

pub fn run(args: &SyncArgs) -> Result<(), Box<dyn std::error::Error>> {
    let schema = Schema::from_file(&args.schema)?;
    let plan = sync::compute_sync_plan(&args.dir, &schema)?;

    match args.format.as_str() {
        "json" => {
            println!("{}", serde_json::to_string_pretty(&plan.to_json())?);
        }
        _ => {
            print!("{}", plan.to_report());
        }
    }

    if !args.dry_run && !plan.is_empty() {
        sync::apply_sync_plan(&plan)?;
        if args.format != "json" {
            println!("Done.");
        }
    } else if args.dry_run && !plan.is_empty() && args.format != "json" {
        println!("Dry run — no files modified.");
    }

    Ok(())
}
