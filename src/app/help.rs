// SPDX-License-Identifier: MPL-2.0
//! Workflow prose for Runyte's contextual help, negotiated through `view-help`.
//!
//! Topics explain what a page is for and how its pages connect. They never
//! list actions or keys: the host generates those from the live registry, so
//! a table here would be a second copy that could drift from the menu.
use super::Content;
use serde_json::{Value, json};

const TOPICS: &[(&str, &str, &[&str])] = &[
    (
        "databases",
        "Databases",
        &[
            "This page lists saved database profiles and whether each one is connected. Enter on a profile connects it, or opens its tables once it is connected.",
            "Add database creates a profile for an existing SQLite file or a PostgreSQL server. Profiles are saved without passwords: PostgreSQL asks for one when connecting, or reads the environment variable its profile names.",
            "At most two databases can be connected at a time, each running one operation at a time. Back from any later page leads here again, and never disconnects.",
        ],
    ),
    (
        "tables",
        "Tables",
        &[
            "This page lists the tables of one connected database; PostgreSQL tables carry their schema. Enter opens the first page of the selected table's rows.",
            "Inspect schema shows the selected table's columns, keys and indexes without reading its rows. System tables stay hidden until they are asked for.",
            "The lines above the list describe the connection, including whether it is read-only. Back returns to Databases.",
        ],
    ),
    (
        "rows",
        "Rows",
        &[
            "This page shows one page of a table, or the rows a query returned. The labelled lines above the rows say which rows are shown, the page size, and any filters and sort; they are not rows themselves.",
            "Enter opens the selected row as a record. Next page and Previous page move through a table one page at a time, and Page size changes how many rows a page holds. Paging keeps the current filters, sort and columns.",
            "Edit filters opens a draft of this table's filters, and Sort rows orders the rows by one column. Applying either returns to the first page. Choose columns hides columns from this page only.",
            "Open current browse as SQL copies the query behind this page into an SQL document to edit and run. Returning to a page shows the rows captured when it was read; it never runs SQL again.",
        ],
    ),
    (
        "record",
        "Record",
        &[
            "This page lists every field of one row, one field per line.",
            "Enter opens the selected field's value. Long values are shown as previews; Show full value, when it is offered, loads the complete captured value into one read-only document.",
            "Back returns to the page of rows this record came from, with its selection restored if the page has not changed.",
        ],
    ),
    (
        "value",
        "Value",
        &[
            "This page shows one field's value. JSON is shown as an outline, and Enter expands or collapses the entry under the cursor.",
            "Show raw value switches between the formatted and the raw text without asking the database again. Show full value loads the complete captured value into one read-only document, where search, selection and copying cover all of it.",
            "The labelled lines above the value give its database type and whether it is a preview or the complete value. Back returns to the record.",
        ],
    ),
    (
        "schema",
        "Schema",
        &[
            "This page lists the selected table's columns, keys and indexes as the database describes them. Enter opens the selected entry as a record.",
            "Back returns to the tables of this database.",
        ],
    ),
    (
        "filters",
        "Filters",
        &[
            "This page is a draft of the filters for one table's rows. Add, edit, enable, disable and remove filters here; the rows page does not change until Apply filters.",
            "Match all or any chooses whether a row has to meet every enabled filter or only one of them. Applying returns to the first page of rows.",
        ],
    ),
    (
        "review",
        "Review",
        &[
            "This page shows captured SQL before it runs. Enter confirms and runs exactly the text shown here; nothing runs before that, and a statement is never run again on its own.",
            "Back leaves the statement without running it.",
        ],
    ),
    (
        "transactions",
        "Transactions",
        &[
            "This page lists connections holding uncommitted changes. Commit and Roll back act on the selected connection's pending transaction and ask for confirmation first.",
        ],
    ),
];

pub(super) const VALUE: &str = "value";

/// Every topic, for the registration message.
pub(super) fn topics() -> Value {
    json!(
        TOPICS
            .iter()
            .map(|(id, title, paragraphs)| json!({"id":id,"title":title,"paragraphs":paragraphs}))
            .collect::<Vec<_>>()
    )
}

/// The topic describing the page a model presents.
pub(super) fn topic(content: &Content) -> &'static str {
    match content {
        Content::Connections => "databases",
        Content::Catalog { .. } => "tables",
        Content::Result {
            record: Some(_), ..
        } => "record",
        Content::Result { source, .. } if source.starts_with("schema:") => "schema",
        Content::Result { .. } => "rows",
        Content::Value { .. } | Content::FullValue { .. } => VALUE,
        Content::Filters { .. } => "filters",
        Content::Review { .. } => "review",
        Content::Transactions { .. } => "transactions",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The host refuses a registration whose topics break these bounds, so a
    /// violation would cost every other command along with help.
    #[test]
    fn topics_stay_within_the_public_bounds() {
        let mut total = 0;
        let mut ids = std::collections::BTreeSet::new();
        assert!(TOPICS.len() <= 16);
        for (id, title, paragraphs) in TOPICS {
            assert!(ids.insert(*id), "{id}");
            assert!(
                id.len() <= 48
                    && id
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            );
            assert!(!title.is_empty() && title.len() <= 64);
            assert!(!paragraphs.is_empty() && paragraphs.len() <= 16);
            for paragraph in *paragraphs {
                assert!(!paragraph.is_empty() && paragraph.len() <= 2048);
                assert!(!paragraph.chars().any(char::is_control));
                // Topics describe workflows; the host lists keys itself.
                assert!(!paragraph.contains("Tab →"), "{paragraph}");
            }
            total += title.len() + paragraphs.iter().map(|p| p.len()).sum::<usize>();
        }
        assert!(total <= 64 * 1024);
    }
}
