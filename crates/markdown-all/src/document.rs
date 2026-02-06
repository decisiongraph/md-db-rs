use std::path::{Path, PathBuf};

use comrak::{Arena, Options};

use crate::ast_util;
use crate::error::{Error, Result};
use crate::frontmatter::Frontmatter;
use crate::section::Section;

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
        let opts = self.comrak_opts();
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
        let opts = self.comrak_opts();
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

    fn comrak_opts(&self) -> Options<'_> {
        let mut opts = Options::default();
        opts.extension.table = true;
        opts
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
}
