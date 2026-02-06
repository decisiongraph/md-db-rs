use comrak::nodes::{AstNode, NodeValue};

use crate::table::Table;

/// Collect plain text from a node (inline only, for heading text etc).
pub fn collect_text<'a>(node: &'a AstNode<'a>) -> String {
    let mut text = String::new();
    collect_text_inner(node, &mut text);
    text
}

fn collect_text_inner<'a>(node: &'a AstNode<'a>, out: &mut String) {
    match &node.data.borrow().value {
        NodeValue::Text(t) => out.push_str(t),
        NodeValue::Code(c) => out.push_str(&c.literal),
        NodeValue::SoftBreak | NodeValue::LineBreak => out.push(' '),
        _ => {}
    }
    for child in node.children() {
        collect_text_inner(child, out);
    }
}

/// Collect plain text with block structure preserved (newlines between paragraphs/headings).
pub fn collect_text_blocks<'a>(node: &'a AstNode<'a>) -> String {
    let mut parts = Vec::new();
    for child in node.children() {
        match &child.data.borrow().value {
            NodeValue::Paragraph
            | NodeValue::Heading(_)
            | NodeValue::CodeBlock(_)
            | NodeValue::List(_)
            | NodeValue::Table(_) => {
                let text = collect_text(child);
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    parts.push(trimmed.to_string());
                }
            }
            _ => {
                let text = collect_text(child);
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    parts.push(trimmed.to_string());
                }
            }
        }
    }
    parts.join("\n\n")
}

/// Find all heading nodes, optionally filtered by level.
pub fn find_headings<'a>(
    root: &'a AstNode<'a>,
    level: Option<u8>,
) -> Vec<&'a AstNode<'a>> {
    let mut headings = Vec::new();
    for node in root.descendants() {
        if let NodeValue::Heading(h) = &node.data.borrow().value {
            if level.is_none() || level == Some(h.level) {
                headings.push(node);
            }
        }
    }
    headings
}

/// Find a heading node by exact text match (case-insensitive).
pub fn find_heading_by_text<'a>(
    root: &'a AstNode<'a>,
    text: &str,
) -> Option<&'a AstNode<'a>> {
    let target = text.trim().to_lowercase();
    for node in root.descendants() {
        if let NodeValue::Heading(_) = &node.data.borrow().value {
            let heading_text = collect_text(node).trim().to_lowercase();
            if heading_text == target {
                return Some(node);
            }
        }
    }
    None
}

/// Get the heading level of a node. Returns None if not a heading.
pub fn heading_level<'a>(node: &'a AstNode<'a>) -> Option<u8> {
    if let NodeValue::Heading(h) = &node.data.borrow().value {
        Some(h.level)
    } else {
        None
    }
}

/// Get the byte range of a section (from heading to next same-or-higher-level heading).
/// Returns (start_byte, end_byte) into the body string.
/// The start includes the heading line itself.
pub fn section_byte_range<'a>(
    heading_node: &'a AstNode<'a>,
    body: &str,
) -> std::ops::Range<usize> {
    let sourcepos = heading_node.data.borrow().sourcepos;
    let level = heading_level(heading_node).unwrap_or(1);

    // Start at the beginning of the heading line (convert 1-based line to byte offset)
    let start = line_col_to_byte(body, sourcepos.start.line, 1);

    // Walk siblings to find the next heading at same or higher level
    let mut next = heading_node.next_sibling();
    while let Some(sibling) = next {
        if let NodeValue::Heading(h) = &sibling.data.borrow().value {
            if h.level <= level {
                let end_pos = sibling.data.borrow().sourcepos;
                let end = line_col_to_byte(body, end_pos.start.line, 1);
                return start..end;
            }
        }
        next = sibling.next_sibling();
    }

    // No next heading found — section extends to end of body
    start..body.len()
}

/// Get byte range of section content (excluding the heading line itself).
pub fn section_content_byte_range<'a>(
    heading_node: &'a AstNode<'a>,
    body: &str,
) -> std::ops::Range<usize> {
    let full_range = section_byte_range(heading_node, body);

    // Skip past the heading line
    let content_start = body[full_range.start..]
        .find('\n')
        .map(|i| full_range.start + i + 1)
        .unwrap_or(full_range.end);

    content_start..full_range.end
}

/// Convert 1-based line number and 1-based column to byte offset.
fn line_col_to_byte(text: &str, line: usize, _col: usize) -> usize {
    let mut current_line = 1;
    for (i, c) in text.char_indices() {
        if current_line == line {
            return i;
        }
        if c == '\n' {
            current_line += 1;
        }
    }
    text.len()
}

/// Find all table nodes in the AST.
pub fn find_tables<'a>(root: &'a AstNode<'a>) -> Vec<&'a AstNode<'a>> {
    let mut tables = Vec::new();
    for node in root.descendants() {
        if let NodeValue::Table(_) = &node.data.borrow().value {
            tables.push(node);
        }
    }
    tables
}

/// Get the byte range of a table node in the body string (sourcepos-based).
pub fn table_byte_range<'a>(
    table_node: &'a AstNode<'a>,
    body: &str,
) -> std::ops::Range<usize> {
    let sourcepos = table_node.data.borrow().sourcepos;
    let start = line_col_to_byte(body, sourcepos.start.line, 1);
    // End at the end of the last line of the table
    let end_line = sourcepos.end.line;
    // Find the byte position at the end of end_line (after the newline)
    let mut current_line = 1;
    let mut end = body.len();
    for (i, c) in body.char_indices() {
        if c == '\n' {
            if current_line == end_line {
                end = i + 1;
                break;
            }
            current_line += 1;
        }
    }
    start..end
}

/// Parse a comrak Table node into our Table struct.
pub fn parse_table_node<'a>(table_node: &'a AstNode<'a>) -> Table {
    let mut headers = Vec::new();
    let mut rows = Vec::new();
    let mut is_header = true;

    for row_node in table_node.children() {
        if let NodeValue::TableRow(header) = &row_node.data.borrow().value {
            is_header = *header;
        }

        let cells: Vec<String> = row_node
            .children()
            .filter(|n| {
                matches!(n.data.borrow().value, NodeValue::TableCell)
            })
            .map(|cell| collect_text(cell).trim().to_string())
            .collect();

        if is_header && headers.is_empty() {
            headers = cells;
        } else {
            rows.push(cells);
        }
    }

    Table::new(headers, rows)
}

#[cfg(test)]
mod tests {
    use comrak::{Arena, Options};

    use super::*;

    #[test]
    fn test_find_headings() {
        let md = "# H1\n\ntext\n\n## H2\n\nmore\n\n# H1b\n";
        let arena = Arena::new();
        let mut opts = Options::default();
        opts.extension.table = true;
        let root = comrak::parse_document(&arena, md, &opts);

        assert_eq!(find_headings(root, None).len(), 3);
        assert_eq!(find_headings(root, Some(1)).len(), 2);
        assert_eq!(find_headings(root, Some(2)).len(), 1);
    }

    #[test]
    fn test_find_heading_by_text() {
        let md = "# Introduction\n\ntext\n\n## Details\n\nmore\n";
        let arena = Arena::new();
        let opts = Options::default();
        let root = comrak::parse_document(&arena, md, &opts);

        assert!(find_heading_by_text(root, "Introduction").is_some());
        assert!(find_heading_by_text(root, "introduction").is_some());
        assert!(find_heading_by_text(root, "details").is_some());
        assert!(find_heading_by_text(root, "missing").is_none());
    }

    #[test]
    fn test_section_byte_range() {
        let md = "# First\n\nContent 1\n\n# Second\n\nContent 2\n";
        let arena = Arena::new();
        let mut opts = Options::default();
        opts.extension.table = true;
        let root = comrak::parse_document(&arena, md, &opts);

        let h = find_heading_by_text(root, "First").unwrap();
        let range = section_byte_range(h, md);
        let section = &md[range];
        assert!(section.contains("Content 1"));
        assert!(!section.contains("Content 2"));
    }

    #[test]
    fn test_parse_table() {
        let md = "| A | B |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n";
        let arena = Arena::new();
        let mut opts = Options::default();
        opts.extension.table = true;
        let root = comrak::parse_document(&arena, md, &opts);

        let tables = find_tables(root);
        assert_eq!(tables.len(), 1);

        let table = parse_table_node(tables[0]);
        assert_eq!(table.headers(), &["A", "B"]);
        assert_eq!(table.get_cell("A", 0), Some("1"));
        assert_eq!(table.get_cell("B", 1), Some("4"));
    }

    #[test]
    fn test_table_byte_range() {
        let md = "# Section\n\nSome text.\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\nMore text.\n";
        let arena = Arena::new();
        let mut opts = Options::default();
        opts.extension.table = true;
        let root = comrak::parse_document(&arena, md, &opts);

        let tables = find_tables(root);
        assert_eq!(tables.len(), 1);

        let range = table_byte_range(tables[0], md);
        let table_text = &md[range];
        assert!(table_text.contains("| A | B |"));
        assert!(table_text.contains("| 1 | 2 |"));
        assert!(!table_text.contains("Some text"));
        assert!(!table_text.contains("More text"));
    }
}
