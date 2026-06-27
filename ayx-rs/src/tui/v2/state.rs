//! Application state. Pure data — no I/O, no rendering.
use crate::tui::v2::context::Context;
use crate::tui::v2::nav::{NavStack, View};
use crate::tui::v2::resource::{Kind, Row};

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
pub struct AppState {
    pub context: Context,
    pub nav: NavStack,
    pub list: ListView,
    pub should_quit: bool,
    pub req_seq: u64,
}

impl AppState {
    pub fn new(context: Context) -> Self {
        Self {
            context,
            nav: NavStack::new(View::ResourceList { kind: Kind::Flow }),
            list: ListView::new(Kind::Flow),
            should_quit: false,
            req_seq: 0,
        }
    }
}
