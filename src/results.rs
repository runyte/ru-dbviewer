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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Representation {
    #[default]
    Text,
    Binary,
    InvalidUtf8,
}
#[derive(Clone, Debug, Default)]
pub enum FullSource {
    #[default]
    Inline,
    Captured(crate::result_storage::CapturedValue),
    Unavailable(String),
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Cell {
    pub text: Option<String>,
    pub truncated: bool,
    #[serde(default)]
    pub representation: Representation,
    #[serde(default)]
    pub complete_size: Option<usize>,
    #[serde(skip)]
    pub full: FullSource,
}
impl Cell {
    /// Preview-only constructor used by synthetic results. Database adapters use capture.
    pub fn new(text: Option<String>) -> Self {
        let size = text.as_ref().map(String::len);
        let truncated = size.is_some_and(|size| size > MAX_VALUE);
        let text = text.map(|mut text| {
            if truncated {
                text.truncate(prefix_end(&text, MAX_VALUE));
            }
            text
        });
        Self {
            text,
            truncated,
            complete_size: size,
            ..Self::default()
        }
    }
    /// Called on the adapter's blocking worker before any preview conversion.
    pub fn capture(text: Option<&str>, capture: &mut crate::result_storage::Capture) -> Self {
        let Some(text) = text else {
            return Self::default();
        };
        let truncated = text.len() > MAX_VALUE;
        let full = if truncated {
            match capture.store(text) {
                Ok(value) => FullSource::Captured(value),
                Err(reason) => FullSource::Unavailable(reason),
            }
        } else {
            FullSource::Inline
        };
        Self {
            text: Some(text[..prefix_end(text, MAX_VALUE)].to_owned()),
            truncated,
            complete_size: Some(text.len()),
            full,
            representation: Representation::Text,
        }
    }
    pub fn capture_hex(
        bytes: &[u8],
        invalid_text: bool,
        capture: &mut crate::result_storage::Capture,
    ) -> Self {
        use crate::result_storage::MAX_FULL_VALUE;
        use std::fmt::Write;
        let (prefix, suffix) = if invalid_text {
            ("[invalid UTF-8 text] x\"", "\"")
        } else {
            ("x'", "'")
        };
        let size = bytes
            .len()
            .checked_mul(2)
            .and_then(|n| n.checked_add(prefix.len() + suffix.len()));
        let fits = size.is_some_and(|n| n <= MAX_FULL_VALUE);
        let count = if fits {
            bytes.len()
        } else {
            (MAX_VALUE - prefix.len()) / 2
        };
        let mut text = String::with_capacity(prefix.len() + count * 2 + suffix.len());
        text.push_str(prefix);
        for (i, byte) in bytes.iter().take(count).enumerate() {
            if i % (16 * 1024) == 0
                && let Err(reason) = capture.check()
            {
                let mut cell = Self::new(Some(text));
                cell.truncated = true;
                cell.complete_size = size;
                cell.representation = if invalid_text {
                    Representation::InvalidUtf8
                } else {
                    Representation::Binary
                };
                cell.full = FullSource::Unavailable(reason);
                return cell;
            }
            write!(&mut text, "{byte:02x}").expect("write to string");
        }
        if fits {
            text.push_str(suffix);
        }
        let mut cell = Self::capture(Some(&text), capture);
        cell.representation = if invalid_text {
            Representation::InvalidUtf8
        } else {
            Representation::Binary
        };
        cell.complete_size = size;
        if !fits {
            cell.truncated = true;
            cell.full = FullSource::Unavailable(
                "Complete hexadecimal value exceeds the 8 MiB limit".into(),
            );
        }
        cell
    }
    pub fn full_available(&self) -> bool {
        match self.full {
            FullSource::Captured(_) => true,
            FullSource::Inline => !self.truncated,
            FullSource::Unavailable(_) => false,
        }
    }
    pub fn unavailable_reason(&self) -> Option<&str> {
        match &self.full {
            FullSource::Unavailable(reason) => Some(reason),
            FullSource::Inline if self.truncated => Some("Complete value was not captured"),
            _ => None,
        }
    }
    pub fn load_full_blocking(&self) -> crate::Result<Option<String>> {
        match &self.full {
            FullSource::Captured(value) => value.read().map(Some),
            FullSource::Inline if !self.truncated => Ok(self.text.clone()),
            _ => Err(self
                .unavailable_reason()
                .unwrap_or("Complete value unavailable")
                .into()),
        }
    }
    pub fn load_full_blocking_checked(
        &self,
        guard: &crate::result_storage::WorkGuard,
    ) -> crate::Result<Option<String>> {
        guard.check()?;
        match &self.full {
            FullSource::Captured(value) => value.read_checked(guard).map(Some),
            _ => self.load_full_blocking(),
        }
    }
    pub async fn load_full(&self) -> crate::Result<Option<String>> {
        let cell = self.clone();
        tokio::task::spawn_blocking(move || cell.load_full_blocking())
            .await
            .map_err(|_| "Full-value storage worker stopped")?
    }
    pub fn display(&self) -> String {
        match &self.text {
            None => "NULL".into(),
            Some(s) if s.is_empty() => "\"\"".into(),
            Some(s) => format!("{}{}", escape(s), if self.truncated { " …" } else { "" }),
        }
    }
}
fn prefix_end(text: &str, limit: usize) -> usize {
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    end
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
    pub affected: Option<u64>,
    pub order_keys: Vec<String>,
    pub truncated: bool,
    pub cells_truncated: bool,
    pub bytes: usize,
}
impl Data {
    pub fn push(&mut self, row: Vec<Cell>) -> bool {
        let size = row
            .iter()
            .map(|c| {
                c.text.as_ref().map_or(0, String::len)
                    + std::mem::size_of::<Cell>()
                    + c.unavailable_reason().map_or(0, str::len)
            })
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

#[cfg(test)]
mod capture_tests {
    use super::*;
    use crate::result_storage::{MAX_FULL_VALUE, Storage};
    #[tokio::test]
    async fn complete_sources_preserve_unicode_null_empty_and_unavailable_state() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::new(dir.path().into());
        let mut capture = storage.result();
        let original = "é".repeat(MAX_VALUE) + "trailing sentinel";
        let cell = Cell::capture(Some(&original), &mut capture);
        assert!(cell.truncated && cell.full_available());
        assert_eq!(cell.complete_size, Some(original.len()));
        assert_eq!(
            cell.load_full().await.unwrap().as_deref(),
            Some(original.as_str())
        );
        assert_eq!(
            Cell::capture(None, &mut capture).load_full().await.unwrap(),
            None
        );
        assert_eq!(
            Cell::capture(Some(""), &mut capture)
                .load_full()
                .await
                .unwrap(),
            Some(String::new())
        );
        let oversized = Cell::capture(Some(&"x".repeat(MAX_FULL_VALUE + 1)), &mut capture);
        assert!(oversized.truncated && !oversized.full_available());
        assert!(oversized.load_full().await.unwrap_err().contains("8 MiB"));
        let serialized = serde_json::to_string(&cell).unwrap();
        let restored: Cell = serde_json::from_str(&serialized).unwrap();
        assert!(!restored.full_available());
        assert!(
            restored
                .unavailable_reason()
                .unwrap()
                .contains("not captured")
        );
    }
    #[test]
    fn binary_and_invalid_utf8_are_complete_or_explicitly_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        let mut capture = Storage::new(dir.path().into()).result();
        let bytes = [0x80, 0xff, 0].repeat(MAX_VALUE);
        for invalid in [false, true] {
            let cell = Cell::capture_hex(&bytes, invalid, &mut capture);
            assert!(cell.truncated && cell.full_available());
            let value = cell.load_full_blocking().unwrap().unwrap();
            assert!(value.ends_with(if invalid { "80ff00\"" } else { "80ff00'" }));
            assert_eq!(value.len(), cell.complete_size.unwrap());
            assert_eq!(
                cell.representation,
                if invalid {
                    Representation::InvalidUtf8
                } else {
                    Representation::Binary
                }
            );
        }
        let too_large = Cell::capture_hex(&vec![0; MAX_FULL_VALUE / 2], false, &mut capture);
        assert!(!too_large.full_available());
        assert!(
            too_large
                .unavailable_reason()
                .unwrap()
                .contains("hexadecimal")
        );
        assert!(too_large.text.unwrap().len() <= MAX_VALUE);
    }
}
