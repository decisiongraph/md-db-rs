use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use clap::Args;
use md_db::document::Document;
use md_db::graph::DocGraph;
use md_db::schema::Schema;
use md_db::users::UserConfig;
use md_db::validation;

#[derive(Debug, Args)]
pub struct StatsArgs {
    /// Directory containing markdown files
    pub dir: PathBuf,

    /// Path to KDL schema file
    #[arg(long)]
    pub schema: PathBuf,

    /// Path to user/team config YAML file
    #[arg(long)]
    pub users: Option<PathBuf>,

    /// Output format: text, json, auto (auto=json when piped)
    #[arg(long, default_value = "auto")]
    pub format: String,
}

pub fn run(args: &StatsArgs) -> Result<(), Box<dyn std::error::Error>> {
    let schema = Schema::from_file(&args.schema)?;
    let user_config = match &args.users {
        Some(path) => Some(UserConfig::from_file(path)?),
        None => None,
    };

    let format = md_db::output::OutputFormat::from_str(&args.format)
        .unwrap_or(md_db::output::OutputFormat::Text);

    // Build graph
    let graph = DocGraph::build(&args.dir, &schema)?;

    // Run validation
    let validation_result =
        validation::validate_directory(&args.dir, &schema, None, user_config.as_ref())?;

    // Aggregate by_type: { type_name -> { total, by_status: { status -> count } } }
    let mut by_type: BTreeMap<String, TypeStats> = BTreeMap::new();
    let files = md_db::discovery::discover_files(&args.dir, None, &[], false)?;
    for path in &files {
        let doc = match Document::from_file(path) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let fm = match &doc.frontmatter {
            Some(fm) => fm,
            None => continue,
        };
        let type_name = match fm.get_display("type") {
            Some(t) => t,
            None => continue,
        };
        let entry = by_type.entry(type_name).or_insert_with(TypeStats::default);
        entry.total += 1;
        if let Some(status) = fm.get_display("status") {
            *entry.by_status.entry(status).or_insert(0) += 1;
        }
    }

    let total_docs = by_type.values().map(|t| t.total).sum::<usize>();

    // Validation summary
    let ok_count = validation_result
        .file_results
        .iter()
        .filter(|fr| fr.errors() == 0)
        .count();
    let error_file_count = validation_result
        .file_results
        .iter()
        .filter(|fr| fr.errors() > 0)
        .count();

    let mut by_code: BTreeMap<String, usize> = BTreeMap::new();
    for fr in &validation_result.file_results {
        for d in &fr.diagnostics {
            *by_code.entry(d.code.clone()).or_insert(0) += 1;
        }
    }

    // Graph stats
    let node_count = graph.nodes.len();
    let edge_count = graph.edges.len();

    // Orphans: nodes with 0 in + 0 out edges
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut out_degree: HashMap<&str, usize> = HashMap::new();
    for edge in &graph.edges {
        *out_degree.entry(edge.from.as_str()).or_insert(0) += 1;
        *in_degree.entry(edge.to.as_str()).or_insert(0) += 1;
    }
    let orphans: Vec<&str> = graph
        .nodes
        .keys()
        .filter(|id| {
            in_degree.get(id.as_str()).copied().unwrap_or(0) == 0
                && out_degree.get(id.as_str()).copied().unwrap_or(0) == 0
        })
        .map(|s| s.as_str())
        .collect();

    // Most referenced (highest in-degree)
    let most_referenced = graph
        .nodes
        .keys()
        .max_by_key(|id| in_degree.get(id.as_str()).copied().unwrap_or(0))
        .filter(|id| in_degree.get(id.as_str()).copied().unwrap_or(0) > 0);

    // Most referencing (highest out-degree)
    let most_referencing = graph
        .nodes
        .keys()
        .max_by_key(|id| out_degree.get(id.as_str()).copied().unwrap_or(0))
        .filter(|id| out_degree.get(id.as_str()).copied().unwrap_or(0) > 0);

    // Staleness: oldest and newest by file mtime
    let mut file_times: Vec<(&str, std::time::SystemTime, &PathBuf)> = Vec::new();
    for (id, node) in &graph.nodes {
        if let Ok(meta) = std::fs::metadata(&node.path) {
            if let Ok(mtime) = meta.modified() {
                file_times.push((id.as_str(), mtime, &node.path));
            }
        }
    }
    file_times.sort_by_key(|(_, t, _)| *t);

    let oldest = file_times.first();
    let newest = file_times.last();

    match format {
        md_db::output::OutputFormat::Json => {
            let mut json = serde_json::Map::new();
            json.insert("total_docs".into(), serde_json::json!(total_docs));

            // by_type
            let bt: serde_json::Map<String, serde_json::Value> = by_type
                .iter()
                .map(|(name, stats)| {
                    (
                        name.clone(),
                        serde_json::json!({
                            "total": stats.total,
                            "by_status": stats.by_status,
                        }),
                    )
                })
                .collect();
            json.insert("by_type".into(), serde_json::Value::Object(bt));

            // validation
            json.insert(
                "validation".into(),
                serde_json::json!({
                    "ok": ok_count,
                    "errors": error_file_count,
                    "by_code": by_code,
                }),
            );

            // graph
            let mut graph_obj = serde_json::json!({
                "nodes": node_count,
                "edges": edge_count,
                "orphans": orphans.len(),
            });
            if let Some(id) = most_referenced {
                graph_obj["most_referenced"] = serde_json::json!({
                    "id": id,
                    "backlinks": in_degree.get(id.as_str()).copied().unwrap_or(0),
                });
            }
            if let Some(id) = most_referencing {
                graph_obj["most_referencing"] = serde_json::json!({
                    "id": id,
                    "outgoing": out_degree.get(id.as_str()).copied().unwrap_or(0),
                });
            }
            json.insert("graph".into(), graph_obj);

            // staleness
            let mut staleness = serde_json::Map::new();
            if let Some((id, time, _)) = oldest {
                staleness.insert(
                    "oldest".into(),
                    serde_json::json!({
                        "id": id,
                        "date": format_system_time(time),
                    }),
                );
            }
            if let Some((id, time, _)) = newest {
                staleness.insert(
                    "newest".into(),
                    serde_json::json!({
                        "id": id,
                        "date": format_system_time(time),
                    }),
                );
            }
            json.insert("staleness".into(), serde_json::Value::Object(staleness));

            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::Value::Object(json))?
            );
        }
        _ => {
            // Text dashboard
            println!("Documents: {total_docs}");
            for (name, stats) in &by_type {
                let status_parts: Vec<String> = stats
                    .by_status
                    .iter()
                    .map(|(s, c)| format!("{c} {s}"))
                    .collect();
                if status_parts.is_empty() {
                    println!("  {name}: {}", stats.total);
                } else {
                    println!(
                        "  {name}: {} ({})",
                        stats.total,
                        status_parts.join(", ")
                    );
                }
            }

            println!();
            println!("Validation: {ok_count} ok, {error_file_count} with errors");
            for (code, count) in &by_code {
                println!("  {code}: {count}");
            }

            println!();
            println!("Graph: {node_count} nodes, {edge_count} edges");
            println!("  Orphans (no refs in or out): {}", orphans.len());
            if let Some(id) = most_referenced {
                let count = in_degree.get(id.as_str()).copied().unwrap_or(0);
                println!("  Most referenced: {id} ({count} backlinks)");
            }
            if let Some(id) = most_referencing {
                let count = out_degree.get(id.as_str()).copied().unwrap_or(0);
                println!("  Most referencing: {id} ({count} outgoing)");
            }

            println!();
            println!("Staleness:");
            if let Some((id, time, _)) = oldest {
                println!("  Oldest unchanged: {id} ({})", format_system_time(time));
            }
            if let Some((id, time, _)) = newest {
                println!("  Newest: {id} ({})", format_system_time(time));
            }
        }
    }

    Ok(())
}

#[derive(Default)]
struct TypeStats {
    total: usize,
    by_status: BTreeMap<String, usize>,
}

fn format_system_time(time: &std::time::SystemTime) -> String {
    let duration = time
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs() as i64;

    // Simple date formatting without external deps
    let days = secs / 86400;
    let mut y = 1970i64;
    let mut remaining_days = days;

    loop {
        let days_in_year = if is_leap_year(y) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        y += 1;
    }

    let month_days = if is_leap_year(y) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut m = 0;
    for (i, &md) in month_days.iter().enumerate() {
        if remaining_days < md {
            m = i + 1;
            break;
        }
        remaining_days -= md;
    }

    let d = remaining_days + 1;
    format!("{y:04}-{m:02}-{d:02}")
}

fn is_leap_year(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}
