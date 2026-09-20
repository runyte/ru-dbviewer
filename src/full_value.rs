// SPDX-License-Identifier: MPL-2.0
//! Complete, lossless value documents. Formatting never repairs a prefix.
use crate::{
    Result,
    result_storage::{MAX_FULL_VALUE, WorkGuard},
    results::{Cell, Representation},
};
use serde_json::{Value, json, value::RawValue};

pub const MAX_LINES: usize = 250_000;
pub const MAX_MODEL: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct Document {
    pub text: String,
    pub format: &'static str,
    pub note: String,
    pub json: bool,
}

impl Document {
    pub fn load(cell: &Cell, raw: bool, guard: &WorkGuard) -> Result<Self> {
        let source = cell.load_full_blocking_checked(guard)?;
        let Some(source) = source else {
            return Ok(Self {
                text: "NULL".into(),
                format: "NULL",
                note: String::new(),
                json: false,
            });
        };
        guard.check()?;
        let mut format = match cell.representation {
            Representation::Binary => "Hexadecimal",
            Representation::InvalidUtf8 => "Invalid UTF-8 · hexadecimal",
            Representation::Text => "Text",
        };
        let mut note = String::new();
        let mut text = source;
        let is_json = cell.representation == Representation::Text
            && serde_json::from_str::<&RawValue>(&text).is_ok();
        if is_json && !raw {
            match pretty(&text, guard) {
                Ok(formatted) => {
                    text = formatted;
                    format = "JSON";
                }
                Err(error) => {
                    guard.check()?;
                    note = format!("Showing complete raw text: {error}");
                }
            }
        }
        if text
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t')
        {
            let mut escaped = String::new();
            for (i, c) in text.chars().enumerate() {
                if i % 4096 == 0 {
                    guard.check()?;
                }
                if c == '\\' {
                    escaped.push_str("\\\\");
                } else if c.is_control() && c != '\n' && c != '\t' {
                    use std::fmt::Write;
                    write!(&mut escaped, "\\u{{{:x}}}", c as u32).expect("string write");
                } else {
                    escaped.push(c);
                }
                if escaped.len() > MAX_FULL_VALUE {
                    return Err("Complete escaped value exceeds the 8 MiB document limit".into());
                }
            }
            text = escaped;
            format = "Escaped text";
            note = "Control characters use \\u{hex}; literal backslashes are doubled. Copying copies this lossless representation.".into();
        }
        check_size(&text)?;
        guard.check()?;
        Ok(Self {
            text,
            format,
            note,
            json: is_json,
        })
    }

    pub fn model(&self, title: &str, presentation: bool, metadata: bool) -> Value {
        let mut model = json!({"title":crate::views::short(&crate::results::escape(&format!("{title} · {}", self.format)), 150),
            "purpose":"document", "rows":[], "document":self.text, "actions":["raw", "back"]});
        if !self.json {
            model["actions"] = json!(["back"]);
        }
        if !self.note.is_empty() {
            model["status"] = json!({"text":self.note,"role":"muted"});
        }
        if metadata {
            model["metadata"] =
                json!([{"label":"Value","value":format!("Complete · {} bytes",self.text.len())}]);
        }
        if presentation && self.json {
            model["action_presentation"] = json!({"raw":{"label":if self.format=="JSON" {"Show raw text"} else {"Format JSON"}, "group":"Inspect", "order":11, "listed":true}});
        }
        model
    }
}

fn check_size(text: &str) -> Result<()> {
    if text.len() > MAX_FULL_VALUE {
        return Err("Complete value exceeds the 8 MiB document limit".into());
    }
    if text.bytes().filter(|&b| b == b'\n').count() >= MAX_LINES {
        return Err("Complete value exceeds the 250,000-line document limit".into());
    }
    Ok(())
}

/// The parser above validates syntax; this pass changes only whitespace outside
/// strings, preserving duplicate keys and the original spelling of every scalar.
fn pretty(source: &str, guard: &WorkGuard) -> Result<String> {
    let mut out = String::with_capacity(source.len().min(MAX_FULL_VALUE));
    let mut depth = 0usize;
    let mut string = false;
    let mut escaped = false;
    let mut previous = None;
    let mut lines = 1;
    for (i, c) in source.chars().enumerate() {
        if i % 4096 == 0 {
            guard.check()?;
        }
        if !string && c.is_ascii_whitespace() {
            continue;
        }
        if !string && matches!(c, '}' | ']') {
            depth = depth.saturating_sub(1);
            if !matches!(previous, Some('{' | '[')) {
                newline(&mut out, depth, &mut lines);
            }
        } else if !string && matches!(previous, Some('{' | '[' | ',')) {
            newline(&mut out, depth, &mut lines);
        }
        out.push(c);
        if string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                string = false;
            }
        } else {
            match c {
                '"' => string = true,
                '{' | '[' => depth += 1,
                ':' => out.push(' '),
                _ => {}
            }
        }
        // Previous structural character must not be inferred from string contents.
        previous = if string { None } else { Some(c) };
        if out.len() > MAX_FULL_VALUE || lines > MAX_LINES {
            return Err("formatted JSON exceeds the document limit".into());
        }
    }
    Ok(out)
}

fn newline(out: &mut String, depth: usize, lines: &mut usize) {
    out.push('\n');
    for _ in 0..depth {
        out.push_str("  ");
    }
    *lines += 1;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    use tokio_util::sync::CancellationToken;
    fn guard() -> WorkGuard {
        WorkGuard::new(
            CancellationToken::new(),
            Instant::now() + Duration::from_secs(60),
        )
    }
    #[test]
    fn full_json_preserves_duplicate_keys_number_spellings_and_whole_strings() {
        let text = r#"{"a":1e+02,"a":-0.00,"s":"a,b:{[\\\"","nested":[{},[]]}"#;
        let doc = Document::load(&Cell::new(Some(text.into())), false, &guard()).unwrap();
        assert_eq!(doc.format, "JSON");
        assert!(doc.text.contains("1e+02"));
        assert!(doc.text.contains("-0.00"));
        assert_eq!(doc.text.matches("\"a\":").count(), 2);
        assert_eq!(
            serde_json::from_str::<Value>(&doc.text).unwrap(),
            serde_json::from_str::<Value>(text).unwrap()
        );
        assert!(!doc.text.ends_with('\n'));
    }
    #[test]
    fn full_document_exceeds_old_row_limit_and_keeps_raw_exact() {
        let text = format!(
            "[{}]",
            (0..12_000)
                .map(|_| "{\"key\":\"value\"}")
                .collect::<Vec<_>>()
                .join(",")
        );
        let cell = Cell {
            text: Some(text.clone()),
            ..Cell::default()
        };
        let doc = Document::load(&cell, false, &guard()).unwrap();
        assert!(doc.text.lines().count() > 10_000);
        assert_eq!(Document::load(&cell, true, &guard()).unwrap().text, text);
    }
    #[test]
    fn oversized_pretty_expansion_falls_back_to_the_complete_raw_value() {
        let text = format!(
            "{}{}{}",
            "[".repeat(64),
            vec!["0"; 100_000].join(","),
            "]".repeat(64)
        );
        let cell = Cell {
            text: Some(text.clone()),
            ..Cell::default()
        };
        let document = Document::load(&cell, false, &guard()).unwrap();
        assert_eq!(document.text, text);
        assert_eq!(document.format, "Text");
        assert!(document.note.contains("formatted JSON exceeds"));
    }
    #[test]
    fn line_and_escaped_size_limits_and_cancellation_never_return_a_partial_document() {
        for text in ["\n".repeat(MAX_LINES), "\0".repeat(MAX_FULL_VALUE / 4)] {
            assert!(
                Document::load(
                    &Cell {
                        text: Some(text),
                        ..Cell::default()
                    },
                    true,
                    &guard()
                )
                .is_err()
            );
        }
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert!(
            Document::load(
                &Cell::new(Some("text".into())),
                false,
                &WorkGuard::new(cancel, Instant::now() + Duration::from_secs(60))
            )
            .unwrap_err()
            .contains("cancelled")
        );
    }
    #[test]
    fn controls_and_literal_escapes_have_distinct_lossless_spellings() {
        let cell = Cell::new(Some("a\0\\u{0}\r\n\t".into()));
        let doc = Document::load(&cell, true, &guard()).unwrap();
        assert_eq!(doc.format, "Escaped text");
        assert_eq!(doc.text, "a\\u{0}\\\\u{0}\\u{d}\n\t");
        assert_eq!(
            Document::load(&Cell::new(Some("a\n\tb".into())), true, &guard())
                .unwrap()
                .text,
            "a\n\tb"
        );
    }
}
