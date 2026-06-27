//! Navigation stack: drill-down is `push`, back is `pop`, breadcrumb is the
//! rendered path. The root view can never be popped.
use crate::tui::v2::resource::Kind;

#[derive(Debug, Clone)]
pub enum View {
    ResourceList {
        kind: Kind,
    },
    ResourceDetail {
        kind: Kind,
        id: String,
        title: String,
    },
    ScopedList {
        child_kind: Kind,
        parent_kind: Kind,
        parent_id: String,
        parent_title: String,
    },
}

impl View {
    fn crumb(&self) -> String {
        match self {
            View::ResourceList { kind } => kind.name().to_string(),
            View::ResourceDetail { title, .. } => title.clone(),
            View::ScopedList { child_kind, .. } => child_kind.name().to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct NavStack {
    stack: Vec<View>,
}

impl NavStack {
    pub fn new(root: View) -> Self {
        Self { stack: vec![root] }
    }

    pub fn push(&mut self, view: View) {
        self.stack.push(view);
    }

    /// Pop one level. Returns false (and does nothing) if already at the root.
    pub fn pop(&mut self) -> bool {
        if self.stack.len() > 1 {
            self.stack.pop();
            true
        } else {
            false
        }
    }

    pub fn top(&self) -> &View {
        self.stack.last().expect("nav stack is never empty")
    }

    pub fn breadcrumb(&self) -> String {
        self.stack
            .iter()
            .map(View::crumb)
            .collect::<Vec<_>>()
            .join(" › ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::v2::resource::Kind;

    #[test]
    fn root_cannot_be_popped() {
        let mut nav = NavStack::new(View::ResourceList { kind: Kind::Flow });
        assert!(!nav.pop());
        assert!(matches!(nav.top(), View::ResourceList { kind: Kind::Flow }));
    }

    #[test]
    fn push_then_pop_returns_to_root() {
        let mut nav = NavStack::new(View::ResourceList { kind: Kind::Flow });
        nav.push(View::ResourceDetail {
            kind: Kind::Flow,
            id: "fl_1".into(),
            title: "ETL Pipeline".into(),
        });
        assert!(matches!(nav.top(), View::ResourceDetail { .. }));
        assert!(nav.pop());
        assert!(matches!(nav.top(), View::ResourceList { .. }));
    }

    #[test]
    fn breadcrumb_shows_path() {
        let mut nav = NavStack::new(View::ResourceList { kind: Kind::Flow });
        assert_eq!(nav.breadcrumb(), "flows");
        nav.push(View::ResourceDetail {
            kind: Kind::Flow,
            id: "fl_1".into(),
            title: "ETL Pipeline".into(),
        });
        assert_eq!(nav.breadcrumb(), "flows › ETL Pipeline");
    }

    #[test]
    fn scoped_list_crumb_is_child_kind_name() {
        let mut nav = NavStack::new(View::ResourceList { kind: Kind::Flow });
        nav.push(View::ResourceDetail {
            kind: Kind::Flow,
            id: "fl_1".into(),
            title: "ETL Pipeline".into(),
        });
        nav.push(View::ScopedList {
            child_kind: Kind::Job,
            parent_kind: Kind::Flow,
            parent_id: "fl_1".into(),
            parent_title: "ETL Pipeline".into(),
        });
        assert_eq!(nav.breadcrumb(), "flows › ETL Pipeline › jobs");
    }
}
