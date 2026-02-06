use std::collections::BTreeMap;
use std::path::Path;

use comrak::{Arena, Options};
use regex::Regex;

use crate::document::Document;
use crate::graph::{path_to_id, DocGraph};
use crate::schema::Schema;

/// Render a Document's markdown body to HTML using comrak.
fn render_markdown_to_html(body: &str) -> String {
    let arena = Arena::new();
    let mut opts = Options::default();
    opts.extension.table = true;
    opts.extension.strikethrough = true;
    opts.extension.autolink = true;
    opts.render.unsafe_ = true;
    let root = comrak::parse_document(&arena, body, &opts);
    let mut html = Vec::new();
    comrak::format_html(root, &opts, &mut html).unwrap_or_default();
    String::from_utf8_lossy(&html).to_string()
}

/// Build a frontmatter metadata HTML table.
fn frontmatter_table(doc: &Document) -> String {
    let fm = match &doc.frontmatter {
        Some(fm) => fm,
        None => return String::new(),
    };

    let mut html = String::from(
        "<table class=\"metadata\">\n<thead><tr><th>Field</th><th>Value</th></tr></thead>\n<tbody>\n",
    );

    for (key, val) in fm.data() {
        let display = crate::output::yaml_value_display(val);
        html.push_str(&format!(
            "<tr><td>{}</td><td>{}</td></tr>\n",
            htmlescape::encode_minimal(key),
            htmlescape::encode_minimal(&display),
        ));
    }
    html.push_str("</tbody>\n</table>\n");
    html
}

/// Convert cross-document refs (e.g. ADR-001) in HTML to clickable links.
fn linkify_refs(html: &str, known_ids: &[String]) -> String {
    if known_ids.is_empty() {
        return html.to_string();
    }
    // Build pattern like (ADR-001|OPP-002|...)
    let escaped: Vec<String> = known_ids.iter().map(|id| regex::escape(id)).collect();
    let pattern = format!(r"\b({})\b", escaped.join("|"));
    let re = Regex::new(&pattern).unwrap();

    re.replace_all(html, |caps: &regex::Captures| {
        let id = &caps[0];
        let lower = id.to_lowercase();
        format!("<a href=\"{lower}.html\">{id}</a>")
    })
    .to_string()
}

/// Minimal CSS for the exported HTML.
const CSS: &str = r#"
body { font-family: system-ui, -apple-system, sans-serif; max-width: 50rem; margin: 2rem auto; padding: 0 1rem; color: #1a1a1a; line-height: 1.6; }
table { border-collapse: collapse; width: 100%; margin: 1rem 0; }
th, td { border: 1px solid #ddd; padding: 0.5rem; text-align: left; }
th { background: #f5f5f5; }
table.metadata { max-width: 30rem; }
table.metadata th { background: #e8e8e8; }
.status-badge { display: inline-block; padding: 0.15rem 0.5rem; border-radius: 3px; font-size: 0.85rem; font-weight: 600; }
.status-deprecated, .status-superseded { background: #fecaca; color: #991b1b; }
.status-accepted, .status-active, .status-resolved { background: #bbf7d0; color: #166534; }
.status-proposed, .status-draft { background: #fef3c7; color: #92400e; }
.backlinks { margin-top: 2rem; padding: 1rem; background: #f9fafb; border: 1px solid #e5e7eb; border-radius: 4px; }
.backlinks h2 { margin-top: 0; font-size: 1rem; }
a { color: #2563eb; }
nav { margin-bottom: 1rem; font-size: 0.9rem; }
h1 { border-bottom: 1px solid #e5e7eb; padding-bottom: 0.3rem; }
"#;

/// Export a single document to a full HTML page.
pub fn export_html(doc: &Document, known_ids: &[String], backlinks: &[(String, String)]) -> String {
    let title = doc
        .frontmatter
        .as_ref()
        .and_then(|fm| fm.get_display("title"))
        .unwrap_or_else(|| "Untitled".to_string());

    let status = doc
        .frontmatter
        .as_ref()
        .and_then(|fm| fm.get_display("status"));

    let doc_id = doc
        .path
        .as_ref()
        .map(|p| path_to_id(p))
        .unwrap_or_default();

    let fm_html = frontmatter_table(doc);
    let body_html = render_markdown_to_html(&doc.body);
    let body_linked = linkify_refs(&body_html, known_ids);

    let status_badge = status
        .as_ref()
        .map(|s| {
            let class = format!("status-{}", s.to_lowercase());
            format!(" <span class=\"status-badge {class}\">{s}</span>")
        })
        .unwrap_or_default();

    let backlinks_html = if backlinks.is_empty() {
        String::new()
    } else {
        let mut bl = String::from("<div class=\"backlinks\"><h2>Referenced by</h2><ul>\n");
        for (ref_id, ref_relation) in backlinks {
            let lower = ref_id.to_lowercase();
            bl.push_str(&format!(
                "<li><a href=\"{lower}.html\">{ref_id}</a> ({ref_relation})</li>\n"
            ));
        }
        bl.push_str("</ul></div>\n");
        bl
    };

    let encoded_title = htmlescape::encode_minimal(&title);
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{doc_id} — {encoded_title}</title>
<style>{CSS}</style>
</head>
<body>
<nav><a href="index.html">Index</a></nav>
<h1>{doc_id}{status_badge}</h1>
{fm_html}
{body_linked}
{backlinks_html}
</body>
</html>
"#
    )
}

/// Export an index page listing all documents grouped by type.
pub fn export_index(docs: &[(String, &Document)]) -> String {
    // Group by type
    let mut by_type: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();

    for (id, doc) in docs {
        let doc_type = doc
            .frontmatter
            .as_ref()
            .and_then(|fm| fm.get_display("type"))
            .unwrap_or_else(|| "other".to_string());
        let title = doc
            .frontmatter
            .as_ref()
            .and_then(|fm| fm.get_display("title"))
            .unwrap_or_else(|| id.clone());
        by_type
            .entry(doc_type)
            .or_default()
            .push((id.clone(), title));
    }

    let mut body = String::new();
    let total = docs.len();
    body.push_str(&format!("<p>{total} documents</p>\n"));

    for (doc_type, entries) in &by_type {
        let upper_type = doc_type.to_uppercase();
        body.push_str(&format!(
            "<h2>{upper_type} ({})</h2>\n<ul>\n",
            entries.len()
        ));
        for (id, title) in entries {
            let lower = id.to_lowercase();
            let encoded = htmlescape::encode_minimal(title);
            body.push_str(&format!(
                "<li><a href=\"{lower}.html\">{id}</a> — {encoded}</li>\n"
            ));
        }
        body.push_str("</ul>\n");
    }

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Document Index</title>
<style>{CSS}</style>
</head>
<body>
<h1>Document Index</h1>
{body}
</body>
</html>
"#
    )
}

/// Export all documents in a directory to HTML files in output_dir.
/// Returns the number of documents exported.
pub fn export_site(
    dir: impl AsRef<Path>,
    schema: Option<&Schema>,
    output_dir: impl AsRef<Path>,
) -> crate::error::Result<usize> {
    let dir = dir.as_ref();
    let output_dir = output_dir.as_ref();
    std::fs::create_dir_all(output_dir)
        .map_err(|_| crate::error::Error::WriteFailed(output_dir.to_path_buf()))?;

    let files = crate::discovery::discover_files(dir, None, &[], false)?;

    // Load all documents
    let mut docs: Vec<(String, Document)> = Vec::new();
    for path in &files {
        let doc = match Document::from_file(path) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let id = path_to_id(path);
        docs.push((id, doc));
    }

    let known_ids: Vec<String> = docs.iter().map(|(id, _)| id.clone()).collect();

    // Build backlinks map if schema provided
    let mut backlinks_map: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    if let Some(schema) = schema {
        if let Ok(graph) = DocGraph::build(dir, schema) {
            for edge in &graph.edges {
                backlinks_map
                    .entry(edge.to.clone())
                    .or_default()
                    .push((edge.from.clone(), edge.relation.clone()));
            }
        }
    }

    // Export each document
    for (id, doc) in &docs {
        let backlinks = backlinks_map.get(id).cloned().unwrap_or_default();
        let html = export_html(doc, &known_ids, &backlinks);
        let filename = format!("{}.html", id.to_lowercase());
        let out_path = output_dir.join(&filename);
        std::fs::write(&out_path, &html)
            .map_err(|_| crate::error::Error::WriteFailed(out_path.clone()))?;
    }

    // Export index
    let doc_refs: Vec<(String, &Document)> = docs.iter().map(|(id, d)| (id.clone(), d)).collect();
    let index_html = export_index(&doc_refs);
    let index_path = output_dir.join("index.html");
    std::fs::write(&index_path, &index_html)
        .map_err(|_| crate::error::Error::WriteFailed(index_path))?;

    Ok(docs.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_markdown_to_html() {
        let md = "# Hello\n\nWorld **bold**.\n";
        let html = render_markdown_to_html(md);
        assert!(html.contains("<h1>"));
        assert!(html.contains("<strong>bold</strong>"));
    }

    #[test]
    fn test_frontmatter_table() {
        let doc = Document::from_str("---\ntitle: Test\nstatus: accepted\n---\n\nBody\n").unwrap();
        let html = frontmatter_table(&doc);
        assert!(html.contains("title"));
        assert!(html.contains("Test"));
        assert!(html.contains("accepted"));
    }

    #[test]
    fn test_linkify_refs() {
        let html = "<p>See ADR-001 and OPP-002 for details.</p>";
        let ids = vec!["ADR-001".to_string(), "OPP-002".to_string()];
        let result = linkify_refs(html, &ids);
        assert!(result.contains("<a href=\"adr-001.html\">ADR-001</a>"));
        assert!(result.contains("<a href=\"opp-002.html\">OPP-002</a>"));
    }

    #[test]
    fn test_export_html() {
        let doc =
            Document::from_str("---\ntitle: Use Postgres\nstatus: accepted\n---\n\n# Decision\n\nWe use PostgreSQL.\n")
                .unwrap();
        let ids = vec!["ADR-001".to_string()];
        let backlinks = vec![("OPP-001".to_string(), "enables".to_string())];
        let html = export_html(&doc, &ids, &backlinks);
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("Use Postgres"));
        assert!(html.contains("accepted"));
        assert!(html.contains("PostgreSQL"));
        assert!(html.contains("Referenced by"));
        assert!(html.contains("OPP-001"));
    }

    #[test]
    fn test_export_index() {
        let doc1 =
            Document::from_str("---\ntitle: ADR 1\ntype: adr\n---\n\nBody\n").unwrap();
        let doc2 =
            Document::from_str("---\ntitle: OPP 1\ntype: opp\n---\n\nBody\n").unwrap();
        let docs = vec![
            ("ADR-001".to_string(), &doc1),
            ("OPP-001".to_string(), &doc2),
        ];
        let html = export_index(&docs);
        assert!(html.contains("Document Index"));
        assert!(html.contains("ADR-001"));
        assert!(html.contains("OPP-001"));
        assert!(html.contains("2 documents"));
    }

    #[test]
    fn test_export_site() {
        let dir = std::env::temp_dir().join("md_db_export_test");
        let input = dir.join("input");
        let output = dir.join("output");
        std::fs::create_dir_all(&input).unwrap();

        std::fs::write(
            input.join("adr-001.md"),
            "---\ntitle: Test ADR\nstatus: accepted\ntype: adr\n---\n\n# Decision\n\nDone.\n",
        )
        .unwrap();

        let count = export_site(&input, None, &output).unwrap();
        assert_eq!(count, 1);
        assert!(output.join("index.html").exists());
        assert!(output.join("adr-001.html").exists());

        // Cleanup
        std::fs::remove_dir_all(&dir).ok();
    }
}
