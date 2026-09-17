// SPDX-License-Identifier: MPL-2.0
use serde::{Deserialize, Serialize};
pub const MAX_VALUE: usize = 64 * 1024;
pub const MAX_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_ROWS: usize = 1000;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    pub kind: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Cell {
    pub text: Option<String>,
    pub truncated: bool,
}
impl Cell {
    pub fn new(text: Option<String>) -> Self {
        let mut truncated = false;
        let text = text.map(|mut s| {
            if s.len() > MAX_VALUE {
                let mut end = MAX_VALUE;
                while !s.is_char_boundary(end) {
                    end -= 1;
                }
                s.truncate(end);
                truncated = true;
            }
            s
        });
        Self { text, truncated }
    }
    pub fn display(&self) -> String {
        match &self.text {
            None => "NULL".into(),
            Some(s) if s.is_empty() => "\"\"".into(),
            Some(s) => format!(
                "{}{}",
                escape(s),
                if self.truncated {
                    " … [truncated]"
                } else {
                    ""
                }
            ),
        }
    }
}
pub fn escape(s: &str) -> String {
    s.chars()
        .flat_map(|c| {
            if c.is_control() || matches!(c, '\u{2028}' | '\u{2029}') {
                c.escape_default().collect::<Vec<_>>()
            } else {
                vec![c]
            }
        })
        .collect()
}
#[derive(Clone, Debug, Default)]
pub struct Data {
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<Cell>>,
    pub affected: u64,
    pub truncated: bool,
    pub cells_truncated: bool,
    pub bytes: usize,
}
impl Data {
    pub fn push(&mut self, row: Vec<Cell>) -> bool {
        let size = row
            .iter()
            .map(|c| c.text.as_ref().map_or(0, String::len) + 32)
            .sum::<usize>();
        if self.rows.len() >= MAX_ROWS || self.bytes + size > MAX_BYTES {
            self.truncated = true;
            return false;
        }
        self.cells_truncated |= row.iter().any(|c| c.truncated);
        self.bytes += size;
        self.rows.push(row);
        true
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn values_preserve_null_and_precision() {
        assert_eq!(Cell::new(None).display(), "NULL");
        assert_eq!(Cell::new(Some(String::new())).display(), "\"\"");
        assert_eq!(
            Cell::new(Some("9223372036854775807".into())).display(),
            "9223372036854775807"
        );
    }
    #[test]
    fn truncate_on_scalar_boundary_and_escape_controls() {
        let c = Cell::new(Some("é".repeat(MAX_VALUE)));
        assert!(c.truncated);
        assert_eq!(c.text.unwrap().len(), MAX_VALUE);
        assert_eq!(escape("a\n\0"), "a\\n\\u{0}");
    }
}

#[cfg(test)]
mod limit_tests {
    use super::*;
    #[test]
    fn clipped_cells_and_incomplete_results_are_independent() {
        let mut data = Data::default();
        assert!(data.push(vec![Cell::new(Some("x".repeat(MAX_VALUE + 1)))]));
        assert!(data.cells_truncated);
        assert!(!data.truncated);
        while data.push(vec![Cell::new(Some("x".repeat(MAX_VALUE)))]) {}
        assert!(data.truncated);
        assert!(data.bytes <= MAX_BYTES);
        let mut rows = Data::default();
        for _ in 0..MAX_ROWS {
            assert!(rows.push(vec![Cell::new(None)]));
        }
        assert!(!rows.push(vec![Cell::new(None)]));
        assert!(rows.truncated);
        assert!(!rows.cells_truncated);
    }
}
