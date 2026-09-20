// SPDX-License-Identifier: MPL-2.0
//! Read-only JSON inspection; original retained text remains authoritative.
use crate::{results::Cell, views};
use serde::Deserialize;
use serde::de::{MapAccess, Visitor};
use serde_json::{Value, json, value::RawValue};
use std::collections::BTreeSet;
const MAX_NODES: usize = 4096;
const MAX_DEPTH: usize = 64;
pub fn model(
    name: &str,
    cell: &Cell,
    raw: bool,
    collapsed: &BTreeSet<String>,
    state: &str,
) -> Value {
    let parsed = if !cell.truncated {
        cell.text.as_ref().and_then(|s| parse(s, 0, &mut 0).ok())
    } else {
        None
    };
    let formatted = !raw && parsed.is_some();
    let prefix = (!raw && cell.truncated)
        .then(|| cell.text.as_deref().and_then(format_json_prefix))
        .flatten();
    let mut rows = Vec::new();
    if formatted {
        render(parsed.as_ref().unwrap(), "0", "", 0, collapsed, &mut rows);
        let mut bytes = 0;
        let keep = rows
            .iter()
            .take_while(|row| {
                bytes += row.to_string().len();
                bytes <= 650_000
            })
            .count();
        let clipped = keep < rows.len();
        rows.truncate(keep);
        if clipped || rows.len() >= MAX_NODES {
            rows.push(views::row(
                "limit",
                "Tree display limit reached; use raw for retained text",
            ));
        }
    } else if let Some(prefix) = &prefix {
        rows = prefix.clone();
    } else {
        let text = cell.display();
        let chars = text.chars().collect::<Vec<_>>();
        for (i, part) in chars.chunks(1024).enumerate() {
            rows.push(json!({"id":i.to_string(),"text":part.iter().collect::<String>(),"role":"ordinary"}));
        }
    }
    if prefix.is_some() {
        rows.push(views::row("end", "…"));
    }
    let status = if prefix.is_some() {
        "Preview · indented JSON prefix"
    } else if cell.truncated {
        "Preview"
    } else if formatted {
        "JSON · Enter expands/collapses"
    } else {
        "Raw value · controls escaped"
    };
    views::text_model(
        &format!("[value] {name}"),
        &format!("{state} · {status}"),
        rows,
        &["activate", "raw", "back"],
    )
}
/// Indent a retained prefix without repairing or claiming to parse incomplete JSON.
/// Strings and scalar spellings stay verbatim; no missing delimiters are invented.
fn format_json_prefix(text: &str) -> Option<Vec<Value>> {
    let text = text.trim_start();
    if !text.starts_with(['{', '[']) {
        return None;
    }
    let mut output = String::new();
    let mut stack = Vec::new();
    let mut string = false;
    let mut escaped = false;
    let mut newline = false;
    for c in text.chars() {
        if !string && c.is_ascii_whitespace() {
            continue;
        }
        if !string && matches!(c, '}' | ']') {
            let open = stack.pop()?;
            if (open == '{') != (c == '}') {
                return None;
            }
            newline = true;
        }
        if newline {
            output.push('\n');
            output.push_str(&"  ".repeat(stack.len()));
            newline = false;
        }
        output.push(c);
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
                '{' | '[' => {
                    stack.push(c);
                    if stack.len() > MAX_DEPTH {
                        return None;
                    }
                    newline = true;
                }
                ',' => newline = true,
                ':' => output.push(' '),
                _ => {}
            }
        }
        if output.len() > 250_000 {
            return None;
        }
    }
    let mut rows = Vec::new();
    let mut bytes = 0;
    for line in output.split('\n') {
        // Match raw inspection's chunks without clipping a long retained string.
        let escaped = crate::results::escape(line).chars().collect::<Vec<_>>();
        for chunk in escaped.chunks(1024) {
            let row = json!({"id":rows.len().to_string(),"text":chunk.iter().collect::<String>(),"role":"ordinary"});
            bytes += row.to_string().len();
            if rows.len() >= MAX_NODES || bytes > 650_000 {
                return None;
            }
            rows.push(row);
        }
    }
    Some(rows)
}
enum Node {
    Scalar {
        id: usize,
        text: String,
    },
    Container {
        id: usize,
        array: bool,
        children: Vec<(String, Node)>,
    },
}
struct Entries(Vec<(String, Box<RawValue>)>);
impl<'de> Deserialize<'de> for Entries {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct Object;
        impl<'de> Visitor<'de> for Object {
            type Value = Entries;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a JSON object")
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Entries, A::Error> {
                let mut entries = Vec::new();
                while let Some(pair) = map.next_entry()? {
                    entries.push(pair);
                }
                Ok(Entries(entries))
            }
        }
        d.deserialize_map(Object)
    }
}
fn parse(text: &str, depth: usize, nodes: &mut usize) -> std::result::Result<Node, ()> {
    let id = *nodes;
    *nodes += 1;
    if depth > MAX_DEPTH || *nodes > MAX_NODES {
        return Err(());
    }
    let raw = serde_json::from_str::<Box<RawValue>>(text).map_err(|_| ())?;
    let text = raw.get();
    let array = text.starts_with('[');
    if array || text.starts_with('{') {
        let entries = if array {
            serde_json::from_str::<Vec<Box<RawValue>>>(text)
                .map_err(|_| ())?
                .into_iter()
                .enumerate()
                .map(|(i, v)| (i.to_string(), v))
                .collect()
        } else {
            serde_json::from_str::<Entries>(text).map_err(|_| ())?.0
        };
        let children = entries
            .into_iter()
            .map(|(key, value)| Ok((key, parse(value.get(), depth + 1, nodes)?)))
            .collect::<std::result::Result<Vec<_>, ()>>()?;
        Ok(Node::Container {
            id,
            array,
            children,
        })
    } else {
        Ok(Node::Scalar {
            id,
            text: text.into(),
        })
    }
}
fn render(
    v: &Node,
    _path: &str,
    label: &str,
    level: usize,
    collapsed: &BTreeSet<String>,
    rows: &mut Vec<Value>,
) {
    if rows.len() >= MAX_NODES {
        return;
    }
    let path = match v {
        Node::Scalar { id, .. } | Node::Container { id, .. } => id.to_string(),
    };
    let indent = "  ".repeat(level);
    let (container, summary) = match v {
        Node::Scalar { text, .. } => (false, text.clone()),
        Node::Container {
            array, children, ..
        } => (
            true,
            if *array {
                format!("[{} items]", children.len())
            } else {
                format!("{{{} fields}}", children.len())
            },
        ),
    };
    rows.push(views::row(
        &path,
        format!(
            "{indent}{}{label}{summary}",
            if container {
                if collapsed.contains(&path) {
                    "▸ "
                } else {
                    "▾ "
                }
            } else {
                ""
            }
        ),
    ));
    if collapsed.contains(&path) {
        return;
    }
    if let Node::Container {
        array, children, ..
    } = v
    {
        for (i, (key, child)) in children.iter().enumerate() {
            let label = if *array {
                key.clone()
            } else {
                serde_json::to_string(key).unwrap()
            };
            render(
                child,
                &format!("{path}.{i}"),
                &format!("{label}: "),
                level + 1,
                collapsed,
                rows,
            );
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn incomplete_json_indents_without_repairing_strings_or_numbers() {
        let text =
            r#"{"x":-0,"x":1.234567890123456789e999,"a":[{"s":"é \",:[]{} \\"},"unfinished\"#;
        let cell = Cell {
            text: Some(text.into()),
            truncated: true,
            ..Cell::default()
        };
        let m = model("x", &cell, false, &BTreeSet::new(), "");
        assert!(
            m["status"]["text"]
                .as_str()
                .unwrap()
                .contains("indented JSON prefix")
        );
        let lines = m["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["text"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            lines,
            vec![
                "{",
                "  \"x\": -0,",
                "  \"x\": 1.234567890123456789e999,",
                "  \"a\": [",
                "    {",
                r#"      "s": "é \",:[]{} \\""#,
                "    },",
                r#"    "unfinished\"#,
                "…"
            ]
        );
        let raw = model("x", &cell, true, &BTreeSet::new(), "");
        assert_eq!(raw["rows"][0]["text"], format!("{text} …"));
    }
    #[test]
    fn incomplete_json_keeps_long_strings_and_bounds_expansion() {
        let cell = Cell::new(Some(format!(r#"{{"s":"{}"}}"#, "é".repeat(40_000))));
        assert!(cell.truncated);
        let m = model("x", &cell, false, &BTreeSet::new(), "");
        let shown = m["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["text"].as_str().unwrap())
            .collect::<String>();
        assert_eq!(
            shown,
            cell.text
                .unwrap()
                .replace(':', ": ")
                .replacen('{', "{  ", 1)
                + "…"
        );
        assert!(m.to_string().len() < 700_000);
        for text in [
            "[".repeat(65),
            format!("[{}", "0,".repeat(5000)),
            "{]".into(),
            "plain text".into(),
        ] {
            assert!(format_json_prefix(&text).is_none());
        }
    }
    #[test]
    fn precision_collapsing_raw_and_incomplete() {
        let cell = Cell::new(Some(
            "{\"n\":123456789012345678901234567890,\"a\":[1,2]}".into(),
        ));
        let model = model("x", &cell, false, &BTreeSet::new(), "");
        assert!(model.to_string().contains("123456789012345678901234567890"));
        let collapsed = BTreeSet::from(["0".into()]);
        assert_eq!(
            super::model("x", &cell, false, &collapsed, "")["rows"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert!(
            super::model("x", &cell, true, &collapsed, "")["status"]["text"]
                .as_str()
                .unwrap()
                .contains("Raw value")
        );
        let mut cell = cell;
        cell.truncated = true;
        assert!(
            super::model("x", &cell, false, &collapsed, "")["status"]["text"]
                .as_str()
                .unwrap()
                .contains("Preview")
        );
    }
    #[test]
    fn deep_and_malformed_remain_readable() {
        for text in [
            "{".into(),
            format!("{}0{}", "[".repeat(100), "]".repeat(100)),
        ] {
            assert!(
                model("x", &Cell::new(Some(text)), false, &BTreeSet::new(), "")["status"]["text"]
                    .as_str()
                    .unwrap()
                    .contains("Raw value")
            );
        }
    }
    #[test]
    fn duplicate_keys_and_number_spelling_are_preserved() {
        let cell = Cell::new(Some(r#"{"x":-0,"x":1.234567890123456789e999}"#.into()));
        let m = model("x", &cell, false, &BTreeSet::new(), "");
        assert_eq!(m["rows"].as_array().unwrap().len(), 3);
        assert!(m.to_string().contains("1.234567890123456789e999"));
        assert!(m.to_string().contains("-0"));
    }
    #[test]
    fn deep_broad_json_stays_within_encoded_model_budget() {
        let text = format!(
            "{}[{}]{}",
            "[".repeat(60),
            vec!["0"; 3000].join(","),
            "]".repeat(60)
        );
        let m = model(
            "bounded",
            &Cell::new(Some(text)),
            false,
            &BTreeSet::new(),
            "",
        );
        assert!(m.to_string().len() < 700_000);
        assert!(
            m["rows"]
                .as_array()
                .unwrap()
                .iter()
                .all(|r| r["id"].as_str().unwrap().len() <= 64)
        );
    }
}
