use std::ops::Range;
use std::path::{Path, PathBuf};

use comrak::Arena;
use serde_yaml::Value;

use crate::ast_util;
use crate::error::{Error, Result};
use crate::frontmatter::Frontmatter;
use crate::section::Section;
use crate::table::Table;

#[derive(Debug, Clone)]
pub struct Document {
    pub path: Option<PathBuf>,
    pub raw: String,
    pub frontmatter: Option<Frontmatter>,
    pub body: String,
}

impl Document {
    /// Load a document from a file path.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            return Err(Error::FileNotFound(path.to_path_buf()));
        }
        let raw = std::fs::read_to_string(path)?;
        let mut doc = Self::from_str(&raw)?;
        doc.path = Some(path.to_path_buf());
        Ok(doc)
    }

    /// Parse a document from a string.
    pub fn from_str(content: &str) -> Result<Self> {
        let (frontmatter, body) = Frontmatter::try_parse(content)?;
        Ok(Self {
            path: None,
            raw: content.to_string(),
            frontmatter,
            body,
        })
    }

    /// Get the frontmatter, returning error if absent.
    pub fn frontmatter(&self) -> Result<&Frontmatter> {
        self.frontmatter.as_ref().ok_or(Error::NoFrontmatter)
    }

    /// Get a section by heading text (case-insensitive exact match).
    pub fn get_section(&self, heading: &str) -> Result<Section> {
        let arena = Arena::new();
        let opts = ast_util::comrak_opts();
        let root = comrak::parse_document(&arena, &self.body, &opts);

        let heading_node = ast_util::find_heading_by_text(root, heading)
            .ok_or_else(|| Error::SectionNotFound(heading.to_string()))?;

        let level = ast_util::heading_level(heading_node).unwrap_or(1);
        let range = ast_util::section_byte_range(heading_node, &self.body);
        let raw = self.body[range.clone()].to_string();
        let content_range = ast_util::section_content_byte_range(heading_node, &self.body);
        let content = self.body[content_range].to_string();

        Ok(Section::new(
            ast_util::collect_text(heading_node),
            level,
            raw,
            content,
        ))
    }

    /// Get a nested section by path, e.g. ["Consequences", "Positive"].
    pub fn get_section_by_path(&self, path: &[&str]) -> Result<Section> {
        if path.is_empty() {
            return Err(Error::SectionNotFound("(empty path)".to_string()));
        }

        let mut section = self.get_section(path[0])?;
        for &name in &path[1..] {
            let sub = section
                .subsections()
                .into_iter()
                .find(|s| s.heading.trim().eq_ignore_ascii_case(name))
                .ok_or_else(|| Error::SectionNotFound(name.to_string()))?;
            section = sub;
        }
        Ok(section)
    }

    /// Get all top-level sections (headings at the minimum level found in the doc).
    pub fn sections(&self) -> Vec<Section> {
        let arena = Arena::new();
        let opts = ast_util::comrak_opts();
        let root = comrak::parse_document(&arena, &self.body, &opts);

        // Find minimum heading level to determine "top-level"
        let all_headings = ast_util::find_headings(root, None);
        let min_level = all_headings
            .iter()
            .filter_map(|n| ast_util::heading_level(n))
            .min()
            .unwrap_or(1);

        let mut sections = Vec::new();
        for node in &all_headings {
            let level = ast_util::heading_level(node).unwrap_or(1);
            if level == min_level {
                let heading_text = ast_util::collect_text(node);
                let range = ast_util::section_byte_range(node, &self.body);
                let raw = self.body[range.clone()].to_string();
                let content_range = ast_util::section_content_byte_range(node, &self.body);
                let content = self.body[content_range].to_string();
                sections.push(Section::new(heading_text, level, raw, content));
            }
        }

        sections
    }

    /// Convert entire document to JSON.
    pub fn to_json(&self) -> serde_json::Value {
        let mut obj = serde_json::Map::new();

        if let Some(ref fm) = self.frontmatter {
            obj.insert("frontmatter".to_string(), fm.to_json());
        }

        if let Some(ref p) = self.path {
            obj.insert(
                "path".to_string(),
                serde_json::Value::String(p.display().to_string()),
            );
        }

        let sections: Vec<serde_json::Value> = self
            .sections()
            .iter()
            .map(|s| {
                let mut sec = serde_json::Map::new();
                sec.insert(
                    "heading".to_string(),
                    serde_json::Value::String(s.heading.clone()),
                );
                sec.insert(
                    "level".to_string(),
                    serde_json::Value::Number(s.level.into()),
                );
                sec.insert(
                    "content".to_string(),
                    serde_json::Value::String(s.content.clone()),
                );
                serde_json::Value::Object(sec)
            })
            .collect();

        obj.insert("sections".to_string(), serde_json::Value::Array(sections));
        obj.insert(
            "body".to_string(),
            serde_json::Value::String(self.body.clone()),
        );

        serde_json::Value::Object(obj)
    }

    // ─── Mutation methods ─────────────────────────────────────────────────

    /// Set a frontmatter field, creating frontmatter if absent.
    pub fn set_field(&mut self, key: &str, value: Value) {
        match self.frontmatter.as_mut() {
            Some(fm) => fm.set(key, value),
            None => {
                let mut fm = Frontmatter::from_data(std::collections::BTreeMap::new());
                fm.set(key, value);
                self.frontmatter = Some(fm);
            }
        }
        self.rebuild_raw();
    }

    /// Parse a string value and set the frontmatter field.
    pub fn set_field_from_str(&mut self, key: &str, raw: &str) {
        let value = crate::frontmatter::parse_yaml_value(raw);
        self.set_field(key, value);
    }

    /// Remove a frontmatter field and rebuild raw content.
    pub fn remove_field(&mut self, key: &str) -> Option<Value> {
        let removed = self.frontmatter.as_mut().and_then(|fm| fm.remove(key));
        if removed.is_some() {
            self.rebuild_raw();
        }
        removed
    }

    /// Replace the content of a section (everything between heading and next heading).
    pub fn replace_section_content(&mut self, heading: &str, new_content: &str) -> Result<()> {
        let range = {
            let arena = Arena::new();
            let opts = ast_util::comrak_opts();
            let root = comrak::parse_document(&arena, &self.body, &opts);
            let heading_node = ast_util::find_heading_by_text(root, heading)
                .ok_or_else(|| Error::SectionNotFound(heading.to_string()))?;
            ast_util::section_content_byte_range(heading_node, &self.body)
        };
        self.replace_body_range(range, new_content);
        Ok(())
    }

    /// Append content at the end of a section (before the next same-or-higher-level heading).
    pub fn append_to_section(&mut self, heading: &str, content: &str) -> Result<()> {
        let range = {
            let arena = Arena::new();
            let opts = ast_util::comrak_opts();
            let root = comrak::parse_document(&arena, &self.body, &opts);
            let heading_node = ast_util::find_heading_by_text(root, heading)
                .ok_or_else(|| Error::SectionNotFound(heading.to_string()))?;
            ast_util::section_content_byte_range(heading_node, &self.body)
        };
        let existing = self.body[range.clone()].to_string();
        let mut new = existing.trim_end().to_string();
        if !new.is_empty() {
            new.push_str("\n\n");
        }
        new.push_str(content);
        new.push('\n');
        self.replace_body_range(range, &new);
        Ok(())
    }

    /// Update a table cell within a section.
    pub fn set_table_cell(
        &mut self,
        heading: &str,
        table_idx: usize,
        col: &str,
        row: usize,
        value: &str,
    ) -> Result<()> {
        let (range, mut table) = self.find_table_byte_range(heading, table_idx)?;
        table.set_cell(col, row, value.to_string())?;
        self.replace_body_range(range, &table.to_markdown());
        Ok(())
    }

    /// Add a row to a table within a section.
    pub fn add_table_row(
        &mut self,
        heading: &str,
        table_idx: usize,
        values: Vec<String>,
    ) -> Result<()> {
        let (range, mut table) = self.find_table_byte_range(heading, table_idx)?;
        table.add_row(values);
        self.replace_body_range(range, &table.to_markdown());
        Ok(())
    }

    /// Save to the document's path (errors if no path set).
    pub fn save(&self) -> Result<()> {
        let path = self.path.as_ref().ok_or(Error::NoPath)?;
        std::fs::write(path, &self.raw).map_err(|_| Error::WriteFailed(path.clone()))?;
        Ok(())
    }

    /// Save to an explicit path.
    pub fn save_to(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        std::fs::write(path, &self.raw).map_err(|_| Error::WriteFailed(path.to_path_buf()))?;
        Ok(())
    }

    /// Reconstruct raw from frontmatter + body.
    fn rebuild_raw(&mut self) {
        let mut raw = String::new();
        if let Some(ref fm) = self.frontmatter {
            raw.push_str("---\n");
            raw.push_str(&fm.to_yaml_string());
            raw.push_str("---\n");
        }
        raw.push_str(&self.body);
        self.raw = raw;
    }

    /// Splice body string then rebuild_raw.
    fn replace_body_range(&mut self, range: Range<usize>, replacement: &str) {
        self.body.replace_range(range, replacement);
        self.rebuild_raw();
    }

    /// Find the byte range and parsed Table for the nth table in a section.
    fn find_table_byte_range(
        &self,
        heading: &str,
        table_idx: usize,
    ) -> Result<(Range<usize>, Table)> {
        let arena = Arena::new();
        let opts = ast_util::comrak_opts();
        let root = comrak::parse_document(&arena, &self.body, &opts);

        let heading_node = ast_util::find_heading_by_text(root, heading)
            .ok_or_else(|| Error::SectionNotFound(heading.to_string()))?;

        let section_range = ast_util::section_byte_range(heading_node, &self.body);

        // Find all tables in the section range
        let all_tables = ast_util::find_tables(root);
        let section_tables: Vec<_> = all_tables
            .into_iter()
            .filter(|t| {
                let tr = ast_util::table_byte_range(t, &self.body);
                tr.start >= section_range.start && tr.end <= section_range.end
            })
            .collect();

        let table_node = section_tables
            .get(table_idx)
            .ok_or(Error::TableNotFound(table_idx))?;

        let range = ast_util::table_byte_range(table_node, &self.body);
        let table = ast_util::parse_table_node(table_node);
        Ok((range, table))
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
---
title: Use PostgreSQL
status: accepted
---

# Decision

We will use PostgreSQL.

## Rationale

It's reliable.

# Consequences

Some consequences here.

## Positive

Good things.

## Negative

Bad things.
";

    #[test]
    fn test_from_str() {
        let doc = Document::from_str(SAMPLE).unwrap();
        assert!(doc.frontmatter.is_some());
        assert_eq!(
            doc.frontmatter().unwrap().get_display("status").unwrap(),
            "accepted"
        );
    }

    #[test]
    fn test_get_section() {
        let doc = Document::from_str(SAMPLE).unwrap();
        let section = doc.get_section("Decision").unwrap();
        assert!(section.content.contains("PostgreSQL"));
        assert!(section.content.contains("Rationale"));
    }

    #[test]
    fn test_get_section_by_path() {
        let doc = Document::from_str(SAMPLE).unwrap();
        let section = doc.get_section_by_path(&["Consequences", "Positive"]).unwrap();
        assert!(section.content.contains("Good things"));
    }

    #[test]
    fn test_sections() {
        let doc = Document::from_str(SAMPLE).unwrap();
        let sections = doc.sections();
        assert_eq!(sections.len(), 2); // Decision, Consequences (top-level = h1)
    }

    #[test]
    fn test_to_json() {
        let doc = Document::from_str(SAMPLE).unwrap();
        let json = doc.to_json();
        assert!(json["frontmatter"]["title"] == "Use PostgreSQL");
    }

    #[test]
    fn test_set_field() {
        let mut doc = Document::from_str(SAMPLE).unwrap();
        doc.set_field("status", serde_yaml::Value::String("deprecated".into()));
        assert_eq!(
            doc.frontmatter().unwrap().get_display("status").unwrap(),
            "deprecated"
        );
        // raw should contain the new value
        assert!(doc.raw.contains("deprecated"));
    }

    #[test]
    fn test_set_field_from_str() {
        let mut doc = Document::from_str(SAMPLE).unwrap();
        doc.set_field_from_str("status", "rejected");
        assert_eq!(
            doc.frontmatter().unwrap().get_display("status").unwrap(),
            "rejected"
        );
    }

    #[test]
    fn test_replace_section_content() {
        let mut doc = Document::from_str(SAMPLE).unwrap();
        doc.replace_section_content("Decision", "New decision text.\n")
            .unwrap();
        let section = doc.get_section("Decision").unwrap();
        assert!(section.content.contains("New decision text"));
        assert!(!section.content.contains("PostgreSQL"));
    }

    #[test]
    fn test_append_to_section() {
        let mut doc = Document::from_str(SAMPLE).unwrap();
        doc.append_to_section("Decision", "Extra note.").unwrap();
        let section = doc.get_section("Decision").unwrap();
        assert!(section.content.contains("PostgreSQL"));
        assert!(section.content.contains("Extra note."));
    }

    const TABLE_DOC: &str = "\
---
title: Tables
---

# Data

| A | B |
|---|---|
| 1 | 2 |
| 3 | 4 |

# Other

Done.
";

    #[test]
    fn test_set_table_cell() {
        let mut doc = Document::from_str(TABLE_DOC).unwrap();
        doc.set_table_cell("Data", 0, "B", 0, "99").unwrap();
        let section = doc.get_section("Data").unwrap();
        let tables = section.tables();
        assert_eq!(tables[0].get_cell("B", 0), Some("99"));
    }

    #[test]
    fn test_add_table_row() {
        let mut doc = Document::from_str(TABLE_DOC).unwrap();
        doc.add_table_row("Data", 0, vec!["5".into(), "6".into()])
            .unwrap();
        let section = doc.get_section("Data").unwrap();
        let tables = section.tables();
        assert_eq!(tables[0].rows().len(), 3);
        assert_eq!(tables[0].get_cell("A", 2), Some("5"));
    }

    #[test]
    fn test_save_to() {
        let doc = Document::from_str(SAMPLE).unwrap();
        let dir = std::env::temp_dir();
        let path = dir.join("md_db_test_save.md");
        doc.save_to(&path).unwrap();
        let loaded = Document::from_file(&path).unwrap();
        assert_eq!(
            loaded.frontmatter().unwrap().get_display("title").unwrap(),
            "Use PostgreSQL"
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_save_no_path_errors() {
        let doc = Document::from_str(SAMPLE).unwrap();
        assert!(doc.save().is_err());
    }
}
