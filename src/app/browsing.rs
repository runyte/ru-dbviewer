// SPDX-License-Identifier: MPL-2.0
use super::*;
use crate::browse::{Filter, Operator};
impl App {
    pub(super) async fn column_picker(
        &mut self,
        ctx: &Value,
        view: String,
        selected: Vec<usize>,
        search: String,
        start: usize,
    ) -> Result<()> {
        let Content::Result { data, .. } = &self.views.get(&view).ok_or("Rows closed")?.content
        else {
            return Err("Open rows first".into());
        };
        let matches = data
            .columns
            .iter()
            .enumerate()
            .filter(|(_, c)| c.name.to_lowercase().contains(&search.to_lowercase()))
            .collect::<Vec<_>>();
        let choices = matches
            .iter()
            .skip(start)
            .take(58)
            .map(|(i, c)| {
                (
                    format!(
                        "[{}] {}: {}",
                        if selected.contains(i) { "x" } else { " " },
                        i + 1,
                        views::short(
                            &crate::results::escape(if c.name.is_empty() {
                                "(unnamed)"
                            } else {
                                &c.name
                            }),
                            120
                        )
                    ),
                    *i,
                )
            })
            .collect::<Vec<_>>();
        let mut labels = vec!["Apply selections".into(), "Find column…".into()];
        labels.extend(choices.iter().map(|(s, _)| s.clone()));
        if start > 0 {
            labels.push("Previous columns…".into());
        }
        if start + 58 < matches.len() {
            labels.push("More columns…".into());
        }
        self.pick(
            ctx,
            &format!("Columns · {} selected (maximum 8)", selected.len()),
            labels,
            Input::ColumnPick {
                view,
                selected,
                search,
                start,
                choices,
            },
        )
        .await
    }
    fn filter_choices(&self, view: &str) -> Result<Vec<(String, usize)>> {
        let v = self.views.get(view).ok_or("View closed")?;
        let columns = match &v.content {
            Content::Filters { draft, .. } => &draft.columns,
            _ => &v.browse.columns,
        };
        Ok(columns
            .iter()
            .enumerate()
            .map(|(i, c)| {
                (
                    format!(
                        "{}: {} [{}]",
                        i + 1,
                        views::short(
                            &crate::results::escape(if c.name.is_empty() {
                                "(unnamed)"
                            } else {
                                &c.name
                            }),
                            100
                        ),
                        views::short(&c.kind, 25)
                    ),
                    i,
                )
            })
            .collect())
    }
    async fn field_picker(
        &mut self,
        ctx: &Value,
        view: String,
        index: Option<usize>,
        sorting: bool,
        search: String,
        start: usize,
    ) -> Result<()> {
        let all = self
            .filter_choices(&view)?
            .into_iter()
            .filter(|(s, _)| s.to_lowercase().contains(&search.to_lowercase()))
            .collect::<Vec<_>>();
        let choices = all.iter().skip(start).take(60).cloned().collect::<Vec<_>>();
        let mut labels = vec!["Find column…".into()];
        labels.extend(choices.iter().map(|(s, _)| s.clone()));
        if start > 0 {
            labels.push("Previous columns…".into());
        }
        if start + 60 < all.len() {
            labels.push("More columns…".into());
        }
        self.pick(
            ctx,
            if sorting {
                "Sort column · primary keys break ties"
            } else {
                "Filter column"
            },
            labels,
            Input::FieldChoice {
                view,
                index,
                sorting,
                search,
                start,
                choices,
            },
        )
        .await
    }
    async fn reload_browse(&mut self, ctx: &Value, id: &str) -> Result<Option<String>> {
        let v = self
            .views
            .get(id)
            .ok_or("Browse parent closed; reopen the table")?;
        let Content::Result {
            name,
            table: Some(table),
            ..
        } = &v.content
        else {
            return Err("Open database rows first".into());
        };
        let mut target = ctx.clone();
        target["view"] = json!(id);
        target["model_revision"] = json!(v.revision);
        target["command"] = json!("refresh");
        let result = self
            .load(&target, name.clone(), Some(table.clone()), 0, false, false)
            .await?;
        self.rpc
            .request(
                "pane.show",
                json!({"invocation":ctx["invocation"],"view":id}),
            )
            .await?;
        Ok(result)
    }
    pub(super) async fn browse_command(
        &mut self,
        ctx: &Value,
        command: &str,
    ) -> Result<Option<String>> {
        let (id, content) = self.context_view(ctx)?;
        match command {
            "filters" => {
                let name = content.name().ok_or("Open rows first")?.to_owned();
                let draft = self.views[&id].browse.clone();
                self.create(
                    ctx,
                    Content::Filters {
                        name,
                        source: id,
                        draft,
                    },
                )
                .await?;
            }
            "add-filter" | "edit-filter" => {
                let Content::Filters { draft, .. } = &content else {
                    return Err("Open filters first".into());
                };
                if command == "add-filter" && draft.filters.len() >= 16 {
                    return Err("At most 16 filter conditions".into());
                }
                let index = if command == "edit-filter" {
                    let i = Self::selected(ctx)?;
                    draft.filters.get(i).ok_or("Select a filter")?;
                    Some(i)
                } else {
                    None
                };
                self.field_picker(ctx, id, index, false, String::new(), 0)
                    .await?;
            }
            "remove-filter" | "toggle-filter" | "clear-filters" => {
                let mut content = content;
                let Content::Filters { draft, .. } = &mut content else {
                    return Err("Open filters first".into());
                };
                if command == "clear-filters" {
                    draft.filters.clear();
                } else {
                    let i = Self::selected(ctx)?;
                    let f = draft.filters.get_mut(i).ok_or("Select a filter")?;
                    if command == "toggle-filter" {
                        f.enabled = !f.enabled;
                    } else {
                        draft.filters.remove(i);
                    }
                }
                self.publish(&id, content).await?;
            }
            "match" => {
                self.pick(
                    ctx,
                    "Enabled filters",
                    vec!["Match ALL (AND)".into(), "Match ANY (OR)".into()],
                    Input::Match(id),
                )
                .await?
            }
            "apply-filters" => {
                let Content::Filters { source, draft, .. } = content else {
                    return Err("Open filters first".into());
                };
                let browse = &mut self
                    .views
                    .get_mut(&source)
                    .ok_or("Browse parent closed")?
                    .browse;
                if browse
                    .columns
                    .iter()
                    .map(|c| &c.name)
                    .ne(draft.columns.iter().map(|c| &c.name))
                {
                    return Err("Table columns changed; reopen filters".into());
                }
                browse.filters = draft.filters;
                browse.any = draft.any;
                return self.reload_browse(ctx, &source).await;
            }
            "sort" => {
                self.field_picker(ctx, id, None, true, String::new(), 0)
                    .await?;
            }
            "page-size" => {
                self.form(
                    ctx,
                    "Browse page size (1–100)",
                    vec![field("size", "Rows per page", "text", true)],
                    Input::PageSize(id),
                )
                .await?
            }
            "browse-sql" => {
                let Content::Result {
                    name,
                    table: Some(table),
                    page,
                    ..
                } = content
                else {
                    return Err("Open database rows first".into());
                };
                self.ready(&name)?;
                let keys = self.views[&id].browse.keys.clone();
                let pg = self
                    .saved
                    .profiles
                    .iter()
                    .find(|p| p.name() == name)
                    .is_some_and(Profile::postgres);
                let (sql, _) = self.views[&id]
                    .browse
                    .compile(&table, page, &keys, pg, true)?;
                crate::query::validate(&sql, pg)?;
                self.query_document_text(
                    ctx,
                    name,
                    &format!(
                        "-- Current offset page only; external writes can shift pages.\n{sql};\n"
                    ),
                )
                .await?;
            }
            _ => return Err("Unknown browsing action".into()),
        }
        Ok(None)
    }
    async fn store_filter(&mut self, id: &str, index: Option<usize>, f: Filter) -> Result<()> {
        let mut content = self.views.get(id).ok_or("Filters closed")?.content.clone();
        let Content::Filters { draft, .. } = &mut content else {
            return Err("Filters closed".into());
        };
        draft.check_filter(&f)?;
        if let Some(i) = index {
            *draft.filters.get_mut(i).ok_or("Filter removed")? = f;
        } else {
            if draft.filters.len() >= 16 {
                return Err("At most 16 filters".into());
            }
            draft.filters.push(f);
        }
        self.publish(id, content).await
    }
    pub(super) async fn browse_submit(
        &mut self,
        ctx: &Value,
        next: Input,
    ) -> Result<Option<String>> {
        let choice = ctx["values"]["choice"].as_str().unwrap_or("");
        match next {
            Input::ColumnPick {
                view,
                mut selected,
                search,
                start,
                choices,
            } => {
                match choice {
                    "Apply selections" => {
                        if selected.is_empty() {
                            return Err("Select at least one column".into());
                        }
                        let v = self.views.get_mut(&view).ok_or("Rows closed")?;
                        let mut content = v.content.clone();
                        if let Content::Result { columns, table, .. } = &mut content {
                            *columns = selected.clone();
                            // Column choice is presentation-only; database filtering retains full rows.
                            if table.is_some() {
                                v.browse.selected = selected;
                            }
                            self.publish(&view, content).await?;
                        }
                        return Ok(None);
                    }
                    "Find column…" => {
                        self.form(
                            ctx,
                            "Find column by name",
                            vec![field("search", "Name contains (empty: all)", "text", false)],
                            Input::ColumnSearch { view, selected },
                        )
                        .await?;
                        return Ok(None);
                    }
                    "More columns…" => {
                        self.column_picker(ctx, view, selected, search, start + 58)
                            .await?;
                        return Ok(None);
                    }
                    "Previous columns…" => {
                        self.column_picker(ctx, view, selected, search, start.saturating_sub(58))
                            .await?;
                        return Ok(None);
                    }
                    _ => {
                        let i = choices
                            .iter()
                            .find(|(s, _)| s == choice)
                            .ok_or("Choose a column")?
                            .1;
                        if selected.contains(&i) {
                            selected.retain(|n| *n != i);
                        } else if selected.len() < 8 {
                            selected.push(i);
                        } else {
                            return Err("At most eight visible columns; deselect one first".into());
                        }
                    }
                }
                self.column_picker(ctx, view, selected, search, start)
                    .await?;
            }
            Input::ColumnSearch { view, selected } => {
                self.column_picker(
                    ctx,
                    view,
                    selected,
                    ctx["values"]["search"].as_str().unwrap_or("").into(),
                    0,
                )
                .await?
            }
            Input::FieldSearch {
                view,
                index,
                sorting,
            } => {
                self.field_picker(
                    ctx,
                    view,
                    index,
                    sorting,
                    ctx["values"]["search"].as_str().unwrap_or("").into(),
                    0,
                )
                .await?
            }
            Input::FieldChoice {
                view,
                index,
                sorting,
                search,
                start,
                choices,
            } => {
                match choice {
                    "Find column…" => {
                        self.form(
                            ctx,
                            "Find column by name",
                            vec![field("search", "Name contains (empty: all)", "text", false)],
                            Input::FieldSearch {
                                view,
                                index,
                                sorting,
                            },
                        )
                        .await?;
                        return Ok(None);
                    }
                    "More columns…" => {
                        self.field_picker(ctx, view, index, sorting, search, start + 60)
                            .await?;
                        return Ok(None);
                    }
                    "Previous columns…" => {
                        self.field_picker(
                            ctx,
                            view,
                            index,
                            sorting,
                            search,
                            start.saturating_sub(60),
                        )
                        .await?;
                        return Ok(None);
                    }
                    _ => {}
                }
                let column = choices
                    .iter()
                    .find(|(s, _)| s == choice)
                    .ok_or("Choose a column")?
                    .1;
                if sorting {
                    self.pick(
                        ctx,
                        "Sort direction",
                        vec![
                            "Ascending".into(),
                            "Descending".into(),
                            "Primary keys only".into(),
                        ],
                        Input::SortDirection(view, column),
                    )
                    .await?;
                } else {
                    let Content::Filters { draft, .. } = &self.views[&view].content else {
                        return Err("Filters closed".into());
                    };
                    let choices = crate::browse::operators(&draft.columns[column].kind);
                    self.pick(
                        ctx,
                        "Filter operator",
                        choices,
                        Input::FilterOp {
                            view,
                            index,
                            column,
                        },
                    )
                    .await?;
                }
            }
            Input::FilterOp {
                view,
                index,
                column,
            } => {
                let op = Operator::from_name(choice)?;
                if matches!(op, Operator::Null | Operator::NotNull) {
                    self.store_filter(
                        &view,
                        index,
                        Filter {
                            column,
                            op,
                            value: String::new(),
                            enabled: true,
                        },
                    )
                    .await?;
                } else {
                    self.form(
                        ctx,
                        "Filter value · contains is literal; empty text is an empty string",
                        vec![field(
                            "value",
                            "Value (numeric columns require decimal text)",
                            "text",
                            false,
                        )],
                        Input::FilterValue {
                            view,
                            index,
                            column,
                            op,
                        },
                    )
                    .await?;
                }
            }
            Input::FilterValue {
                view,
                index,
                column,
                op,
            } => {
                self.store_filter(
                    &view,
                    index,
                    Filter {
                        column,
                        op,
                        value: ctx["values"]["value"].as_str().unwrap_or("").into(),
                        enabled: true,
                    },
                )
                .await?
            }
            Input::Match(id) => {
                let mut content = self.views.get(&id).ok_or("Filters closed")?.content.clone();
                let Content::Filters { draft, .. } = &mut content else {
                    return Err("Open filters first".into());
                };
                draft.any = match choice {
                    "Match ALL (AND)" => false,
                    "Match ANY (OR)" => true,
                    _ => return Err("Choose Match ALL or ANY".into()),
                };
                self.publish(&id, content).await?;
            }
            Input::SortDirection(id, column) => {
                self.views.get_mut(&id).ok_or("Rows closed")?.browse.sort = match choice {
                    "Ascending" => Some((column, false)),
                    "Descending" => Some((column, true)),
                    "Primary keys only" => None,
                    _ => return Err("Choose sort direction".into()),
                };
                return self.reload_browse(ctx, &id).await;
            }
            Input::PageSize(id) => {
                let size = ctx["values"]["size"]
                    .as_str()
                    .unwrap_or("")
                    .parse::<usize>()
                    .map_err(|_| "Use a page size from 1 to 100")?;
                if !(1..=100).contains(&size) {
                    return Err("Use a page size from 1 to 100".into());
                }
                self.views
                    .get_mut(&id)
                    .ok_or("Rows closed")?
                    .browse
                    .page_size = size;
                return self.reload_browse(ctx, &id).await;
            }
            _ => return Err("Input expired".into()),
        }
        Ok(None)
    }
}
