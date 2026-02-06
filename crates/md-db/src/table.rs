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

    /// Set a cell value by column name and row index (0-based).
    pub fn set_cell(&mut self, col: &str, row: usize, value: String) -> Result<()> {
        let col_idx = self
            .headers
            .iter()
            .position(|h| h == col)
            .ok_or_else(|| Error::ColumnNotFound(col.to_string()))?;
        let max = self.rows.len();
        let r = self
            .rows
            .get_mut(row)
            .ok_or(Error::RowOutOfBounds { row, max })?;
        if col_idx < r.len() {
            r[col_idx] = value;
        }
        Ok(())
    }

    /// Add a row. Pads or truncates to match header count.
    pub fn add_row(&mut self, values: Vec<String>) {
        let mut row = values;
        row.resize(self.headers.len(), String::new());
        row.truncate(self.headers.len());
        self.rows.push(row);
    }

    /// Render as GFM markdown table.
    pub fn to_markdown(&self) -> String {
        if self.headers.is_empty() {
            return String::new();
        }

        let mut out = String::new();

        // Header row
        out.push_str("| ");
        out.push_str(&self.headers.join(" | "));
        out.push_str(" |\n");

        // Separator
        out.push_str("|");
        for _ in &self.headers {
            out.push_str("---|");
        }
        out.push('\n');

        // Data rows
        for row in &self.rows {
            out.push_str("| ");
            let cells: Vec<&str> = (0..self.headers.len())
                .map(|i| row.get(i).map(|s| s.as_str()).unwrap_or(""))
                .collect();
            out.push_str(&cells.join(" | "));
            out.push_str(" |\n");
        }

        out
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

    #[test]
    fn test_set_cell() {
        let mut t = sample_table();
        t.set_cell("Score", 0, "10".into()).unwrap();
        assert_eq!(t.get_cell("Score", 0), Some("10"));

        // Column not found
        assert!(t.set_cell("Missing", 0, "x".into()).is_err());
        // Row out of bounds
        assert!(t.set_cell("Score", 99, "x".into()).is_err());
    }

    #[test]
    fn test_add_row() {
        let mut t = sample_table();
        t.add_row(vec!["Carol".into(), "9".into()]);
        assert_eq!(t.rows().len(), 3);
        assert_eq!(t.get_cell("Name", 2), Some("Carol"));

        // Pad short row
        t.add_row(vec!["Dave".into()]);
        assert_eq!(t.get_cell("Score", 3), Some(""));

        // Truncate long row
        t.add_row(vec!["Eve".into(), "7".into(), "extra".into()]);
        assert_eq!(t.rows()[4].len(), 2);
    }

    #[test]
    fn test_to_markdown() {
        let t = sample_table();
        let md = t.to_markdown();
        assert!(md.contains("| Name | Score |"));
        assert!(md.contains("|---|---|"));
        assert!(md.contains("| Alice | 8 |"));
        assert!(md.contains("| Bob | 6 |"));
    }
}
