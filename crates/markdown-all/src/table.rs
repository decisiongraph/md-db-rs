use serde_json::Value;

use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct Table {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
}

impl Table {
    pub fn new(headers: Vec<String>, rows: Vec<Vec<String>>) -> Self {
        Self { headers, rows }
    }

    pub fn headers(&self) -> &[String] {
        &self.headers
    }

    pub fn rows(&self) -> &[Vec<String>] {
        &self.rows
    }

    /// Get a cell value by column name and row index (0-based).
    pub fn get_cell(&self, col: &str, row: usize) -> Option<&str> {
        let col_idx = self.headers.iter().position(|h| h == col)?;
        self.rows.get(row)?.get(col_idx).map(|s| s.as_str())
    }

    /// Get a cell, returning an error if not found.
    pub fn get_cell_or_err(&self, col: &str, row: usize) -> Result<&str> {
        self.get_cell(col, row).ok_or(Error::CellNotFound {
            col: col.to_string(),
            row,
        })
    }

    /// Get all values in a column.
    pub fn get_column(&self, col: &str) -> Option<Vec<&str>> {
        let col_idx = self.headers.iter().position(|h| h == col)?;
        Some(
            self.rows
                .iter()
                .filter_map(|row| row.get(col_idx).map(|s| s.as_str()))
                .collect(),
        )
    }

    /// Get a row by index (0-based).
    pub fn get_row(&self, row: usize) -> Option<&[String]> {
        self.rows.get(row).map(|r| r.as_slice())
    }

    /// Convert to JSON: array of objects.
    pub fn to_json(&self) -> Value {
        let arr: Vec<Value> = self
            .rows
            .iter()
            .map(|row| {
                let obj: serde_json::Map<String, Value> = self
                    .headers
                    .iter()
                    .zip(row.iter())
                    .map(|(h, v)| (h.clone(), Value::String(v.clone())))
                    .collect();
                Value::Object(obj)
            })
            .collect();
        Value::Array(arr)
    }

    /// Format as aligned text table.
    pub fn to_text(&self) -> String {
        if self.headers.is_empty() {
            return String::new();
        }

        // Calculate column widths
        let mut widths: Vec<usize> = self.headers.iter().map(|h| h.len()).collect();
        for row in &self.rows {
            for (i, cell) in row.iter().enumerate() {
                if i < widths.len() {
                    widths[i] = widths[i].max(cell.len());
                }
            }
        }

        let mut out = String::new();

        // Header
        let header: Vec<String> = self
            .headers
            .iter()
            .enumerate()
            .map(|(i, h)| format!("{:width$}", h, width = widths[i]))
            .collect();
        out.push_str(&header.join(" | "));
        out.push('\n');

        // Separator
        let sep: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
        out.push_str(&sep.join("-+-"));
        out.push('\n');

        // Rows
        for row in &self.rows {
            let cells: Vec<String> = row
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    let w = widths.get(i).copied().unwrap_or(0);
                    format!("{:width$}", c, width = w)
                })
                .collect();
            out.push_str(&cells.join(" | "));
            out.push('\n');
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_table() -> Table {
        Table::new(
            vec!["Name".into(), "Score".into()],
            vec![
                vec!["Alice".into(), "8".into()],
                vec!["Bob".into(), "6".into()],
            ],
        )
    }

    #[test]
    fn test_get_cell() {
        let t = sample_table();
        assert_eq!(t.get_cell("Score", 0), Some("8"));
        assert_eq!(t.get_cell("Name", 1), Some("Bob"));
        assert_eq!(t.get_cell("Missing", 0), None);
        assert_eq!(t.get_cell("Name", 5), None);
    }

    #[test]
    fn test_get_column() {
        let t = sample_table();
        assert_eq!(t.get_column("Name"), Some(vec!["Alice", "Bob"]));
    }

    #[test]
    fn test_get_row() {
        let t = sample_table();
        assert_eq!(t.get_row(0), Some(["Alice".to_string(), "8".to_string()].as_slice()));
    }

    #[test]
    fn test_to_json() {
        let t = sample_table();
        let json = t.to_json();
        assert_eq!(json[0]["Name"], "Alice");
        assert_eq!(json[1]["Score"], "6");
    }
}
