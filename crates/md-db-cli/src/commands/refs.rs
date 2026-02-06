use std::path::PathBuf;

use clap::Args;
use md_db::graph::{DocGraph, path_to_id};
use md_db::output::OutputFormat;
use md_db::schema::Schema;

#[derive(Debug, Args)]
pub struct RefsArgs {
    /// Directory containing markdown files
    pub dir: PathBuf,

    /// Path to KDL schema file
    #[arg(long)]
    pub schema: PathBuf,

    /// Show outgoing refs from this file or ID
    #[arg(long)]
    pub from: Option<String>,

    /// Show incoming refs (backlinks) to this ID
    #[arg(long)]
    pub to: Option<String>,

    /// Transitive traversal depth (default: 1, i.e. direct only)
    #[arg(long, default_value = "1")]
    pub depth: usize,

    /// Output format: text, json, compact, auto
    #[arg(long, default_value = "auto")]
    pub format: String,
}

pub fn run(args: &RefsArgs) -> Result<(), Box<dyn std::error::Error>> {
    let schema = Schema::from_file(&args.schema)?;
    let graph = DocGraph::build(&args.dir, &schema)?;
    let format = OutputFormat::from_str(&args.format).unwrap_or(OutputFormat::auto());

    if let Some(ref target) = args.to {
        // Backlinks to a document
        let id = normalize_id(target);
        let edges = if args.depth > 1 {
            graph.refs_to_transitive(&id, args.depth)
        } else {
            graph.refs_to(&id).into_iter().map(|e| (1, e)).collect()
        };

        output_edges(&edges, &graph, &id, "backlinks", format);
    } else if let Some(ref source) = args.from {
        // Forward refs from a document
        let id = resolve_id(source);
        let edges = if args.depth > 1 {
            graph.refs_from_transitive(&id, args.depth)
        } else {
            graph.refs_from(&id).into_iter().map(|e| (1, e)).collect()
        };

        output_edges(&edges, &graph, &id, "refs", format);
    } else {
        return Err("specify --from or --to".into());
    }

    Ok(())
}

fn normalize_id(s: &str) -> String {
    s.to_uppercase().replace('_', "-")
}

fn resolve_id(s: &str) -> String {
    // If it looks like a file path, extract ID from it
    if s.contains('/') || s.ends_with(".md") {
        path_to_id(std::path::Path::new(s))
    } else {
        normalize_id(s)
    }
}

fn output_edges(
    edges: &[(usize, &md_db::graph::DocEdge)],
    graph: &DocGraph,
    focus_id: &str,
    mode: &str,
    format: OutputFormat,
) {
    match format {
        OutputFormat::Json => {
            let items: Vec<serde_json::Value> = edges
                .iter()
                .map(|(depth, e)| {
                    let peer_id = if mode == "backlinks" {
                        &e.from
                    } else {
                        &e.to
                    };
                    let node = graph.nodes.get(peer_id);
                    serde_json::json!({
                        "id": peer_id,
                        "relation": e.relation,
                        "depth": depth,
                        "type": node.and_then(|n| n.doc_type.as_deref()),
                        "title": node.and_then(|n| n.title.as_deref()),
                        "status": node.and_then(|n| n.status.as_deref()),
                        "path": node.map(|n| n.path.display().to_string()),
                    })
                })
                .collect();

            let result = serde_json::json!({
                "id": focus_id,
                "mode": mode,
                "results": items,
                "count": items.len(),
            });
            println!("{}", serde_json::to_string_pretty(&result).unwrap());
        }
        OutputFormat::Compact => {
            for (depth, e) in edges {
                let peer_id = if mode == "backlinks" {
                    &e.from
                } else {
                    &e.to
                };
                println!("{}:{}:{}:{}", peer_id, e.relation, depth, mode);
            }
        }
        _ => {
            if edges.is_empty() {
                println!("No {mode} for {focus_id}.");
                return;
            }
            println!("{} for {}:", capitalize(mode), focus_id);
            for (depth, e) in edges {
                let peer_id = if mode == "backlinks" {
                    &e.from
                } else {
                    &e.to
                };
                let node = graph.nodes.get(peer_id);
                let title = node
                    .and_then(|n| n.title.as_deref())
                    .unwrap_or("");
                let indent = "  ".repeat(*depth);
                println!("{indent}{peer_id}  ({})  {title}", e.relation);
            }
        }
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}
