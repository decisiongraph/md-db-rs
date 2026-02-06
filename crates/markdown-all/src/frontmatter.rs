use std::collections::BTreeMap;

use gray_matter::{engine::YAML, Matter};
use serde_yaml::Value;

use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct Frontmatter {
    data: BTreeMap<String, Value>,
}

impl Frontmatter {
    /// Parse frontmatter from raw file content. Returns (Frontmatter, body).
    pub fn parse(raw: &str) -> Result<(Self, String)> {
        let matter = Matter::<YAML>::new();
        let result = matter.parse(raw);

        let data: BTreeMap<String, Value> = match result.data {
            Some(pod) => pod
                .deserialize()
                .map_err(|e| Error::FrontmatterParse(e.to_string()))?,
            None => return Err(Error::NoFrontmatter),
        };

        Ok((Self { data }, result.content))
    }

    /// Try to parse frontmatter; returns (None, full_content) if no frontmatter found.
    pub fn try_parse(raw: &str) -> Result<(Option<Self>, String)> {
        match Self::parse(raw) {
            Ok((fm, body)) => Ok((Some(fm), body)),
            Err(Error::NoFrontmatter) => Ok((None, raw.to_string())),
            Err(e) => Err(e),
        }
    }

    /// Get a value by dotted path (e.g. "links.superseded_by").
    pub fn get(&self, path: &str) -> Option<&Value> {
        let parts: Vec<&str> = path.split('.').collect();
        let mut current: &Value = self.data.get(parts[0])?;

        for part in &parts[1..] {
            match current {
                Value::Mapping(map) => {
                    current = map.get(Value::String(part.to_string()))?;
                }
                _ => return None,
            }
        }

        Some(current)
    }

    /// Get all keys at the top level.
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.data.keys()
    }

    /// Check if a top-level field exists.
    pub fn has_field(&self, key: &str) -> bool {
        self.data.contains_key(key)
    }

    /// Get the underlying data map.
    pub fn data(&self) -> &BTreeMap<String, Value> {
        &self.data
    }

    /// Serialize to YAML string.
    pub fn to_yaml(&self) -> std::result::Result<String, serde_yaml::Error> {
        serde_yaml::to_string(&self.data)
    }

    /// Convert to JSON value.
    pub fn to_json(&self) -> serde_json::Value {
        yaml_to_json(&Value::Mapping(
            self.data
                .iter()
                .map(|(k, v)| (Value::String(k.clone()), v.clone()))
                .collect(),
        ))
    }

    /// Get a field value as a plain string (for display).
    pub fn get_display(&self, path: &str) -> Option<String> {
        self.get(path).map(yaml_value_to_string)
    }
}

fn yaml_value_to_string(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        Value::Sequence(seq) => {
            let items: Vec<String> = seq.iter().map(yaml_value_to_string).collect();
            format!("[{}]", items.join(", "))
        }
        Value::Mapping(_) => serde_yaml::to_string(v).unwrap_or_default(),
        Value::Tagged(tagged) => yaml_value_to_string(&tagged.value),
    }
}

fn yaml_to_json(v: &Value) -> serde_json::Value {
    match v {
        Value::Null => serde_json::Value::Null,
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                serde_json::Value::Number(i.into())
            } else if let Some(f) = n.as_f64() {
                serde_json::Number::from_f64(f)
                    .map(serde_json::Value::Number)
                    .unwrap_or(serde_json::Value::Null)
            } else {
                serde_json::Value::Null
            }
        }
        Value::String(s) => serde_json::Value::String(s.clone()),
        Value::Sequence(seq) => {
            serde_json::Value::Array(seq.iter().map(yaml_to_json).collect())
        }
        Value::Mapping(map) => {
            let obj: serde_json::Map<String, serde_json::Value> = map
                .iter()
                .filter_map(|(k, v)| {
                    let key = match k {
                        Value::String(s) => s.clone(),
                        other => yaml_value_to_string(other),
                    };
                    Some((key, yaml_to_json(v)))
                })
                .collect();
            serde_json::Value::Object(obj)
        }
        Value::Tagged(tagged) => yaml_to_json(&tagged.value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_frontmatter() {
        let content = "---\ntitle: Test\nstatus: accepted\n---\n\n# Body\n";
        let (fm, body) = Frontmatter::parse(content).unwrap();
        assert_eq!(fm.get_display("title").unwrap(), "Test");
        assert_eq!(fm.get_display("status").unwrap(), "accepted");
        assert!(body.contains("# Body"));
    }

    #[test]
    fn test_dotted_path() {
        let content = "---\nlinks:\n  superseded_by: ADR-005\n---\nbody";
        let (fm, _) = Frontmatter::parse(content).unwrap();
        assert_eq!(fm.get_display("links.superseded_by").unwrap(), "ADR-005");
    }

    #[test]
    fn test_no_frontmatter() {
        let content = "# Just a heading\n\nNo frontmatter here.";
        let (fm, body) = Frontmatter::try_parse(content).unwrap();
        assert!(fm.is_none());
        assert!(body.contains("Just a heading"));
    }

    #[test]
    fn test_to_json() {
        let content = "---\ntitle: Test\ncount: 42\ntags:\n  - a\n  - b\n---\nbody";
        let (fm, _) = Frontmatter::parse(content).unwrap();
        let json = fm.to_json();
        assert_eq!(json["title"], "Test");
        assert_eq!(json["count"], 42);
        assert_eq!(json["tags"][0], "a");
    }

    #[test]
    fn test_has_field() {
        let content = "---\ntitle: Test\n---\nbody";
        let (fm, _) = Frontmatter::parse(content).unwrap();
        assert!(fm.has_field("title"));
        assert!(!fm.has_field("missing"));
    }
}
