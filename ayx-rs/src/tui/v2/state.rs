//! Application state. Pure data — no I/O, no rendering.
use crate::tui::v2::context::Context;
use crate::tui::v2::nav::{NavStack, View};
use crate::tui::v2::resource::{Kind, Row};
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct ListView {
    pub kind: Kind,
    pub rows: Vec<Row>,
    pub cursor: usize,
    pub loading: bool,
    pub error: Option<String>,
    pub token: u64,
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
        }
    }

    pub fn select_down(&mut self) {
        if !self.rows.is_empty() && self.cursor + 1 < self.rows.len() {
            self.cursor += 1;
        }
    }

    pub fn select_up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn selected(&self) -> Option<&Row> {
        self.rows.get(self.cursor)
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
