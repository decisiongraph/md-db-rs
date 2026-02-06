use std::path::PathBuf;

use clap::Args;
use md_db::graph::DocGraph;
use md_db::schema::Schema;

#[derive(Debug, Args)]
pub struct GraphArgs {
    /// Directory containing markdown files
    pub dir: PathBuf,

    /// Path to KDL schema file
    #[arg(long)]
    pub schema: PathBuf,

    /// Output format: mermaid, dot, json
    #[arg(long, default_value = "mermaid")]
    pub format: String,

    /// Filter by document type
    #[arg(long = "type")]
    pub doc_type: Option<String>,
}

pub fn run(args: &GraphArgs) -> Result<(), Box<dyn std::error::Error>> {
    let schema = Schema::from_file(&args.schema)?;
    let graph = DocGraph::build(&args.dir, &schema)?;
    let filter_type = args.doc_type.as_deref();

    match args.format.as_str() {
        "mermaid" => {
            print!("{}", graph.to_mermaid(filter_type));
        }
        "dot" => {
            print!("{}", graph.to_dot(filter_type));
        }
        "json" => {
            let nodes: Vec<serde_json::Value> = graph
                .nodes
                .values()
                .filter(|n| {
                    filter_type
                        .map(|ft| n.doc_type.as_deref() == Some(ft))
                        .unwrap_or(true)
                })
                .map(|n| {
                    serde_json::json!({
                        "id": n.id,
                        "type": n.doc_type,
                        "title": n.title,
                        "status": n.status,
                        "path": n.path.display().to_string(),
                    })
                })
                .collect();

            let edges: Vec<serde_json::Value> = graph
                .edges
                .iter()
                .map(|e| {
                    serde_json::json!({
                        "from": e.from,
                        "to": e.to,
                        "relation": e.relation,
                    })
                })
                .collect();

            let result = serde_json::json!({
                "nodes": nodes,
                "edges": edges,
                "node_count": nodes.len(),
                "edge_count": edges.len(),
            });
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        other => {
            return Err(format!("unknown format \"{other}\", expected mermaid, dot, or json").into());
        }
    }

    Ok(())
}
