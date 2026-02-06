use std::path::PathBuf;

use clap::Args;
use md_db::document::Document;
use md_db::graph::{DocGraph, path_to_id};
use md_db::schema::Schema;

#[derive(Debug, Args)]
pub struct DeprecateArgs {
    /// Path to the markdown file to deprecate
    pub file: PathBuf,

    /// Path to KDL schema file
    #[arg(long)]
    pub schema: PathBuf,

    /// Mark as superseded by this document ID (sets status=superseded + superseded_by field)
    #[arg(long)]
    pub superseded_by: Option<String>,

    /// Directory to scan for updating backlinks (optional)
    #[arg(long)]
    pub dir: Option<PathBuf>,

    /// Print result to stdout instead of writing files
    #[arg(long)]
    pub dry_run: bool,
}

pub fn run(args: &DeprecateArgs) -> Result<(), Box<dyn std::error::Error>> {
    let schema = Schema::from_file(&args.schema)?;
    let mut doc = Document::from_file(&args.file)?;
    let doc_id = path_to_id(&args.file);

    if let Some(ref replacement_id) = args.superseded_by {
        // Set status=superseded and add superseded_by field
        doc.set_field_from_str("status", "superseded");
        doc.set_field_from_str("superseded_by", replacement_id);
        eprintln!("{doc_id}: status=superseded, superseded_by={replacement_id}");
    } else {
        // Just deprecate
        doc.set_field_from_str("status", "deprecated");
        eprintln!("{doc_id}: status=deprecated");
    }

    if args.dry_run {
        print!("{}", doc.raw);
    } else {
        doc.save()?;

        // If --dir is provided, scan for backlinks and add a warning
        if let Some(ref dir) = args.dir {
            let graph = DocGraph::build(dir, &schema)?;
            let backlinks = graph.refs_to(&doc_id);

            for edge in &backlinks {
                if graph.nodes.get(&edge.from).is_none() {
                    continue;
                }
                // Skip self-references
                if edge.from == doc_id {
                    continue;
                }
                eprintln!(
                    "  backlink: {} ({}) references deprecated {doc_id}",
                    edge.from, edge.relation
                );
            }

            if !backlinks.is_empty() {
                eprintln!(
                    "  {} document(s) still reference {doc_id}",
                    backlinks.len()
                );
            }
        }
    }

    Ok(())
}
