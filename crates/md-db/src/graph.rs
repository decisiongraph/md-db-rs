use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use crate::document::Document;
use crate::error::Result;
use crate::schema::Schema;

/// A node in the document graph.
#[derive(Debug, Clone)]
pub struct DocNode {
    /// Canonical ID derived from filename (e.g. "ADR-001")
    pub id: String,
    pub path: PathBuf,
    pub doc_type: Option<String>,
    pub title: Option<String>,
    pub status: Option<String>,
}

/// A directed edge (reference) between two documents.
#[derive(Debug, Clone)]
pub struct DocEdge {
    pub from: String,
    pub to: String,
    /// The relation field name (e.g. "supersedes", "enables", "related")
    pub relation: String,
}

/// The document graph built from a directory of markdown files.
#[derive(Debug)]
pub struct DocGraph {
    pub nodes: BTreeMap<String, DocNode>,
    pub edges: Vec<DocEdge>,
}

impl DocGraph {
    /// Build a graph from all markdown files in a directory.
    pub fn build(dir: impl AsRef<Path>, schema: &Schema) -> Result<Self> {
        let files = crate::discovery::discover_files(&dir, None, &[])?;
        let relation_names = schema.all_relation_field_names();

        let mut nodes = BTreeMap::new();
        let mut edges = Vec::new();

        for path in &files {
            let doc = match Document::from_file(path) {
                Ok(d) => d,
                Err(_) => continue,
            };

            let id = path_to_id(path);
            let fm = match &doc.frontmatter {
                Some(fm) => fm,
                None => {
                    // Check if this is a singleton type
                    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    let singleton_type = schema.types.iter().find(|t| {
                        t.singleton && t.match_pattern.as_deref() == Some(filename)
                    });
                    if let Some(type_def) = singleton_type {
                        let id = path_to_id(path);
                        nodes.insert(
                            id.clone(),
                            DocNode {
                                id: id.clone(),
                                path: path.clone(),
                                doc_type: Some(type_def.name.clone()),
                                title: None,
                                status: None,
                            },
                        );
                    }
                    continue;
                }
            };

            let doc_type = fm.get_display("type");
            let title = fm.get_display("title");
            let status = fm.get_display("status");

            nodes.insert(
                id.clone(),
                DocNode {
                    id: id.clone(),
                    path: path.clone(),
                    doc_type,
                    title,
                    status,
                },
            );

            // Extract outgoing refs from relation fields
            for rel_name in &relation_names {
                if let Some(val) = fm.get(rel_name) {
                    let refs = extract_refs(val);
                    for target in refs {
                        edges.push(DocEdge {
                            from: id.clone(),
                            to: target,
                            relation: rel_name.to_string(),
                        });
                    }
                }
            }
        }

        Ok(DocGraph { nodes, edges })
    }

    /// Get all outgoing refs from a document.
    pub fn refs_from(&self, id: &str) -> Vec<&DocEdge> {
        let id_upper = id.to_uppercase();
        self.edges
            .iter()
            .filter(|e| e.from == id_upper)
            .collect()
    }

    /// Get all incoming refs (backlinks) to a document.
    pub fn refs_to(&self, id: &str) -> Vec<&DocEdge> {
        let id_upper = id.to_uppercase();
        self.edges
            .iter()
            .filter(|e| e.to == id_upper)
            .collect()
    }

    /// Transitive forward refs from a document up to a depth limit.
    /// Returns (depth, edge) pairs.
    pub fn refs_from_transitive(&self, id: &str, max_depth: usize) -> Vec<(usize, &DocEdge)> {
        let mut result = Vec::new();
        let mut visited = HashSet::new();
        let mut queue = vec![(id.to_uppercase(), 0usize)];

        while let Some((current, depth)) = queue.pop() {
            if depth >= max_depth {
                continue;
            }
            for edge in self.refs_from(&current) {
                if visited.insert((edge.from.clone(), edge.to.clone(), edge.relation.clone())) {
                    result.push((depth + 1, edge));
                    queue.push((edge.to.clone(), depth + 1));
                }
            }
        }

        result
    }

    /// Transitive backlinks to a document up to a depth limit.
    pub fn refs_to_transitive(&self, id: &str, max_depth: usize) -> Vec<(usize, &DocEdge)> {
        let mut result = Vec::new();
        let mut visited = HashSet::new();
        let mut queue = vec![(id.to_uppercase(), 0usize)];

        while let Some((current, depth)) = queue.pop() {
            if depth >= max_depth {
                continue;
            }
            for edge in self.refs_to(&current) {
                if visited.insert((edge.from.clone(), edge.to.clone(), edge.relation.clone())) {
                    result.push((depth + 1, edge));
                    queue.push((edge.from.clone(), depth + 1));
                }
            }
        }

        result
    }

    /// Export graph as mermaid diagram.
    pub fn to_mermaid(&self, filter_type: Option<&str>) -> String {
        let mut out = String::from("graph LR\n");

        // Node declarations
        for (id, node) in &self.nodes {
            if let Some(ft) = filter_type {
                if node.doc_type.as_deref() != Some(ft) {
                    continue;
                }
            }
            let label = node
                .title
                .as_deref()
                .unwrap_or(id.as_str());
            let shape = if node.status.as_deref() == Some("deprecated")
                || node.status.as_deref() == Some("superseded")
            {
                format!("  {id}[/\"{label}\"/]")
            } else {
                format!("  {id}[\"{label}\"]")
            };
            out.push_str(&shape);
            out.push('\n');
        }

        // Edges
        let active_ids: HashSet<&str> = if let Some(ft) = filter_type {
            self.nodes
                .iter()
                .filter(|(_, n)| n.doc_type.as_deref() == Some(ft))
                .map(|(id, _)| id.as_str())
                .collect()
        } else {
            self.nodes.keys().map(|s| s.as_str()).collect()
        };

        for edge in &self.edges {
            if !active_ids.contains(edge.from.as_str()) && filter_type.is_some() {
                continue;
            }
            // Only show edges where both endpoints are in scope, or target is external
            let label = &edge.relation;
            out.push_str(&format!(
                "  {} -->|{}| {}\n",
                edge.from, label, edge.to
            ));
        }

        out
    }

    /// Export graph as DOT (graphviz) format.
    pub fn to_dot(&self, filter_type: Option<&str>) -> String {
        let mut out = String::from("digraph docs {\n  rankdir=LR;\n  node [shape=box];\n\n");

        for (id, node) in &self.nodes {
            if let Some(ft) = filter_type {
                if node.doc_type.as_deref() != Some(ft) {
                    continue;
                }
            }
            let label = node.title.as_deref().unwrap_or(id.as_str());
            let style = if node.status.as_deref() == Some("deprecated")
                || node.status.as_deref() == Some("superseded")
            {
                " style=dashed"
            } else {
                ""
            };
            out.push_str(&format!("  \"{id}\" [label=\"{label}\"{style}];\n"));
        }

        out.push('\n');

        let active_ids: HashSet<&str> = if let Some(ft) = filter_type {
            self.nodes
                .iter()
                .filter(|(_, n)| n.doc_type.as_deref() == Some(ft))
                .map(|(id, _)| id.as_str())
                .collect()
        } else {
            self.nodes.keys().map(|s| s.as_str()).collect()
        };

        for edge in &self.edges {
            if !active_ids.contains(edge.from.as_str()) && filter_type.is_some() {
                continue;
            }
            out.push_str(&format!(
                "  \"{}\" -> \"{}\" [label=\"{}\"];\n",
                edge.from, edge.to, edge.relation
            ));
        }

        out.push_str("}\n");
        out
    }

    /// Find next available numeric ID for a type prefix (e.g. "ADR" → "ADR-005").
    pub fn next_id(&self, prefix: &str) -> String {
        let prefix_upper = prefix.to_uppercase();
        let max = self
            .nodes
            .keys()
            .filter_map(|id| {
                let parts: Vec<&str> = id.splitn(2, '-').collect();
                if parts.len() == 2 && parts[0] == prefix_upper {
                    parts[1].parse::<u32>().ok()
                } else {
                    None
                }
            })
            .max()
            .unwrap_or(0);

        format!("{}-{:03}", prefix_upper, max + 1)
    }
}

/// Derive a document ID from its file path.
/// Extracts the type-prefix + number from the filename:
///   `docs/adr-001.md` → `ADR-001`
///   `docs/adr-001-start-using-postgresql.md` → `ADR-001`
///   `docs/inc_002.md` → `INC-002`
pub fn path_to_id(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_uppercase()
        .replace('_', "-");

    // Try to extract PREFIX-NNN from the beginning
    // Match: letters, then dash, then digits
    let bytes = stem.as_bytes();
    let mut i = 0;
    // Skip alpha prefix
    while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
        i += 1;
    }
    // Expect dash
    if i < bytes.len() && bytes[i] == b'-' {
        i += 1;
        let num_start = i;
        // Consume digits
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i > num_start {
            // We found PREFIX-NNN — return just that part
            return stem[..i].to_string();
        }
    }

    // Fallback: use full stem
    stem
}

/// Extract ref strings from a YAML value (single string or array of strings).
fn extract_refs(val: &serde_yaml::Value) -> Vec<String> {
    match val {
        serde_yaml::Value::String(s) => vec![s.to_uppercase()],
        serde_yaml::Value::Sequence(seq) => seq
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_uppercase()))
            .collect(),
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_to_id() {
        assert_eq!(path_to_id(Path::new("docs/adr-001.md")), "ADR-001");
        assert_eq!(path_to_id(Path::new("inc_002.md")), "INC-002");
        assert_eq!(
            path_to_id(Path::new("docs/adr-001-start-using-postgresql.md")),
            "ADR-001"
        );
        assert_eq!(
            path_to_id(Path::new("opp-003-expand-to-europe.md")),
            "OPP-003"
        );
    }

    #[test]
    fn test_build_graph_from_fixtures() {
        let schema_content = std::fs::read_to_string("../../tests/fixtures/schema.kdl").unwrap();
        let schema = Schema::from_str(&schema_content).unwrap();
        let graph = DocGraph::build("../../tests/fixtures", &schema).unwrap();

        // Should have nodes for all fixture docs
        assert!(graph.nodes.contains_key("ADR-001"));
        assert!(graph.nodes.contains_key("OPP-001"));
        assert!(graph.nodes.contains_key("GOV-001"));
        assert!(graph.nodes.contains_key("INC-001"));

        // ADR-001 should have outgoing refs
        let refs = graph.refs_from("ADR-001");
        assert!(!refs.is_empty());
        let targets: Vec<&str> = refs.iter().map(|e| e.to.as_str()).collect();
        assert!(targets.contains(&"OPP-001"), "ADR-001 enables OPP-001");
        assert!(targets.contains(&"GOV-001"), "ADR-001 triggers GOV-001");
    }

    #[test]
    fn test_backlinks() {
        let schema_content = std::fs::read_to_string("../../tests/fixtures/schema.kdl").unwrap();
        let schema = Schema::from_str(&schema_content).unwrap();
        let graph = DocGraph::build("../../tests/fixtures", &schema).unwrap();

        // OPP-001 should have backlink from ADR-001 (enables)
        let backlinks = graph.refs_to("OPP-001");
        let sources: Vec<&str> = backlinks.iter().map(|e| e.from.as_str()).collect();
        assert!(sources.contains(&"ADR-001"));
    }

    #[test]
    fn test_transitive_refs() {
        let schema_content = std::fs::read_to_string("../../tests/fixtures/schema.kdl").unwrap();
        let schema = Schema::from_str(&schema_content).unwrap();
        let graph = DocGraph::build("../../tests/fixtures", &schema).unwrap();

        let transitive = graph.refs_from_transitive("ADR-001", 3);
        assert!(!transitive.is_empty());
        // Should find depth=1 refs and possibly depth=2+
        assert!(transitive.iter().any(|(d, _)| *d == 1));
    }

    #[test]
    fn test_next_id() {
        let schema_content = std::fs::read_to_string("../../tests/fixtures/schema.kdl").unwrap();
        let schema = Schema::from_str(&schema_content).unwrap();
        let graph = DocGraph::build("../../tests/fixtures", &schema).unwrap();

        // Fixtures have adr-001, adr-002, adr-003
        let next = graph.next_id("ADR");
        assert_eq!(next, "ADR-004");

        // Only one OPP fixture
        let next = graph.next_id("OPP");
        assert_eq!(next, "OPP-002");
    }

    #[test]
    fn test_mermaid_output() {
        let schema_content = std::fs::read_to_string("../../tests/fixtures/schema.kdl").unwrap();
        let schema = Schema::from_str(&schema_content).unwrap();
        let graph = DocGraph::build("../../tests/fixtures", &schema).unwrap();

        let mermaid = graph.to_mermaid(None);
        assert!(mermaid.starts_with("graph LR"));
        assert!(mermaid.contains("ADR-001"));
        assert!(mermaid.contains("-->"));
    }

    #[test]
    fn test_dot_output() {
        let schema_content = std::fs::read_to_string("../../tests/fixtures/schema.kdl").unwrap();
        let schema = Schema::from_str(&schema_content).unwrap();
        let graph = DocGraph::build("../../tests/fixtures", &schema).unwrap();

        let dot = graph.to_dot(None);
        assert!(dot.starts_with("digraph docs"));
        assert!(dot.contains("ADR-001"));
        assert!(dot.contains("->"));
    }
}
