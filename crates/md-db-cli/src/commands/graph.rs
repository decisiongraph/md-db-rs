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

    /// Run structural health checks instead of rendering the graph
    #[arg(long)]
    pub check: bool,
}

pub fn run(args: &GraphArgs) -> Result<(), Box<dyn std::error::Error>> {
    let schema = Schema::from_file(&args.schema)?;
    let graph = DocGraph::build(&args.dir, &schema)?;

    if args.check {
        return run_check(&graph, &schema, &args.format);
    }

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

fn run_check(
    graph: &DocGraph,
    schema: &Schema,
    format: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let diags = graph.check_health(schema);

    match format {
        "json" => {
            let items: Vec<serde_json::Value> = diags
                .iter()
                .map(|d| {
                    serde_json::json!({
                        "code": d.code,
                        "severity": d.severity,
                        "message": d.message,
                    })
                })
                .collect();
            let result = serde_json::json!({
                "diagnostics": items,
                "count": items.len(),
            });
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        _ => {
            if diags.is_empty() {
                println!("No issues found.");
            } else {
                for d in &diags {
                    let icon = match d.severity.as_str() {
                        "error" => "ERR ",
                        "warning" => "WARN",
                        "info" => "INFO",
                        _ => "    ",
                    };
                    println!("[{icon}] {}: {}", d.code, d.message);
                }
                println!("\n{} issue(s) found.", diags.len());
            }
        }
    }

    let has_errors = diags.iter().any(|d| d.severity == "error");
    if has_errors {
        std::process::exit(1);
    }

    Ok(())
}
