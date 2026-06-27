//! Application state. Pure data — no I/O, no rendering.
use crate::tui::v2::context::Context;
use crate::tui::v2::nav::{NavStack, View};
use crate::tui::v2::resource::{Kind, Row};
use serde_json::Value;
use tui_input::Input;

#[derive(Debug, Clone)]
pub struct ListView {
    pub kind: Kind,
    pub rows: Vec<Row>,
    pub cursor: usize,
    pub loading: bool,
    pub error: Option<String>,
    pub token: u64,
    pub filter: Input,
    pub filtering: bool,
}

impl ListView {
    pub fn new(kind: Kind) -> Self {
        Self {
            kind,
            rows: Vec::new(),
            cursor: 0,
            loading: true,
            error: None,
            token: 0,
            filter: Input::default(),
            filtering: false,
        }
    }

    pub fn visible(&self) -> Vec<&Row> {
        let term = self.filter.value();
        if term.is_empty() {
            return self.rows.iter().collect();
        }

        let needle = term.to_lowercase();
        self.rows
            .iter()
            .filter(|row| {
                row.cells
                    .first()
                    .map(|cell| cell.text.to_lowercase().contains(&needle))
                    .unwrap_or(false)
            })
            .collect()
    }

    pub fn select_down(&mut self) {
        let len = self.visible().len();
        if len > 0 && self.cursor + 1 < len {
            self.cursor += 1;
        }
    }

    pub fn select_up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn selected(&self) -> Option<&Row> {
        self.visible().get(self.cursor).copied()
    }
}

#[derive(Debug, Clone)]
pub struct DetailView {
    pub kind: Kind,
    pub id: String,
    pub title: String,
    pub loading: bool,
    pub json: Option<Value>,
    pub error: Option<String>,
    pub scroll: u16,
    pub token: u64,
}

impl DetailView {
    pub fn new(kind: Kind, id: String, title: String, token: u64) -> Self {
        Self {
            kind,
            id,
            title,
            loading: true,
            json: None,
            error: None,
            scroll: 0,
            token,
        }
    }

    pub fn scroll_down(&mut self) {
        self.scroll = self.scroll.saturating_add(1);
    }

    pub fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
    }
}

#[derive(Debug, Clone)]
pub struct AppState {
    pub context: Context,
    pub nav: NavStack,
    pub list: ListView,
    pub detail: Option<DetailView>,
    pub should_quit: bool,
    pub req_seq: u64,
}

impl AppState {
    pub fn new(context: Context) -> Self {
        Self {
            context,
            nav: NavStack::new(View::ResourceList { kind: Kind::Flow }),
            list: ListView::new(Kind::Flow),
            detail: None,
            should_quit: false,
            req_seq: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::v2::resource::{Cell, Kind, Row};

    fn lv_with(names: &[&str]) -> ListView {
        let mut lv = ListView::new(Kind::Flow);
        lv.loading = false;
        lv.rows = names
            .iter()
            .map(|n| Row {
                id: n.to_string(),
                cells: vec![Cell::plain(*n)],
            })
            .collect();
        lv
    }

    #[test]
    fn visible_is_all_when_no_filter() {
        let lv = lv_with(&["alpha", "beta"]);
        assert_eq!(lv.visible().len(), 2);
    }

    #[test]
    fn visible_filters_on_input_value() {
        let mut lv = lv_with(&["Daily ETL", "Sales Rollup", "daily report"]);
        lv.filter = tui_input::Input::default().with_value("daily".to_string());
        let vis = lv.visible();
        assert_eq!(vis.len(), 2);
        assert_eq!(vis[0].cells[0].text, "Daily ETL");
    }

    #[test]
    fn selected_indexes_into_visible() {
        let mut lv = lv_with(&["aaa", "bbb", "abc"]);
        lv.filter = tui_input::Input::default().with_value("a".to_string());
        lv.cursor = 1;
        assert_eq!(lv.selected().unwrap().cells[0].text, "abc");
    }
}
