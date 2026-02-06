use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::error::Result;
use crate::frontmatter::Frontmatter;

/// A filter for frontmatter fields.
#[derive(Debug, Clone)]
pub enum Filter {
    /// Field must equal value.
    FieldEquals { key: String, value: String },
    /// Field must NOT equal value.
    FieldNotEquals { key: String, value: String },
    /// Field value must contain substring.
    FieldContains { key: String, value: String },
    /// Field value must be one of these values (comma-separated in CLI).
    FieldIn { key: String, values: Vec<String> },
    /// Field must exist.
    HasField(String),
    /// Field must NOT exist.
    NotHasField(String),
}

/// Discover markdown files in a directory with optional filtering.
pub fn discover_files(
    dir: impl AsRef<Path>,
    pattern: Option<&str>,
    filters: &[Filter],
) -> Result<Vec<PathBuf>> {
    let dir = dir.as_ref();
    let glob_pattern = pattern.unwrap_or("*.md");

    let mut results = Vec::new();

    for entry in WalkDir::new(dir).follow_links(true).into_iter().flatten() {
        let path = entry.path();

        if !path.is_file() {
            continue;
        }

        // Check glob pattern against filename
        if !matches_glob(path, glob_pattern) {
            continue;
        }

        // If there are filters, parse frontmatter and check
        if !filters.is_empty() {
            let content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let fm = match Frontmatter::try_parse(&content) {
                Ok((Some(fm), _)) => fm,
                _ => continue,
            };

            if !check_filters(&fm, filters) {
                continue;
            }
        }

        results.push(path.to_path_buf());
    }

    results.sort();
    Ok(results)
}

fn matches_glob(path: &Path, pattern: &str) -> bool {
    let file_name = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => return false,
    };

    // Use glob::Pattern for matching
    match glob::Pattern::new(pattern) {
        Ok(pat) => pat.matches(file_name),
        Err(_) => false,
    }
}

fn check_filters(fm: &Frontmatter, filters: &[Filter]) -> bool {
    for filter in filters {
        match filter {
            Filter::FieldEquals { key, value } => {
                match fm.get_display(key) {
                    Some(v) if v == *value => {}
                    _ => return false,
                }
            }
            Filter::FieldNotEquals { key, value } => {
                match fm.get_display(key) {
                    Some(v) if v != *value => {}
                    None => {} // field absent counts as "not equal"
                    _ => return false,
                }
            }
            Filter::FieldContains { key, value } => {
                match fm.get_display(key) {
                    Some(v) if v.contains(value.as_str()) => {}
                    _ => return false,
                }
            }
            Filter::FieldIn { key, values } => {
                match fm.get_display(key) {
                    Some(v) if values.iter().any(|allowed| *allowed == v) => {}
                    _ => return false,
                }
            }
            Filter::HasField(key) => {
                if !fm.has_field(key) {
                    return false;
                }
            }
            Filter::NotHasField(key) => {
                if fm.has_field(key) {
                    return false;
                }
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matches_glob() {
        let path = Path::new("docs/adr-001.md");
        assert!(matches_glob(path, "*.md"));
        assert!(matches_glob(path, "adr-*.md"));
        assert!(!matches_glob(path, "*.txt"));
    }
}
