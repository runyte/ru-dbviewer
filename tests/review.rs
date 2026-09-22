// SPDX-License-Identifier: MPL-2.0
use ru_dbviewer::views::Content;

#[test]
fn sql_review_preserves_lines_and_literal_escapes() {
    let sql = "SELECT\n\n  'literal \\n',\t1;\n";
    let content = Content::Review {
        generation: 1,
        name: "fixture".into(),
        sql: sql.into(),
        source: "captured".into(),
    };
    let model = content.model("ready", 100);
    let rows = model["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0]["text"], "SELECT");
    assert_eq!(rows[1]["text"], "");
    assert_eq!(rows[2]["text"], "  'literal \\n',\\t1;");
    assert_eq!(rows[3]["text"], "");
}
