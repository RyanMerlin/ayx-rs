//! Resource model: the k9s engine. Each browsable asset implements
//! `ResourceKind`, so the list/detail views and effect executor are written
//! once and work for every asset. Phase 0 ships `Kind::Flow` only.
use serde_json::Value;

pub mod flow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Flow,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Flow => "flows",
        }
    }

    pub fn all() -> &'static [Kind] {
        &[Kind::Flow]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusTone {
    Neutral,
    Ok,
    Warn,
    Danger,
}

#[derive(Debug, Clone)]
pub struct Cell {
    pub text: String,
    pub tone: StatusTone,
}

impl Cell {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            tone: StatusTone::Neutral,
        }
    }
    pub fn toned(text: impl Into<String>, tone: StatusTone) -> Self {
        Self {
            text: text.into(),
            tone,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Column {
    pub title: &'static str,
    pub width: u16,
}

#[derive(Debug, Clone)]
pub struct Row {
    pub id: String,
    pub cells: Vec<Cell>,
}

#[derive(Debug, Clone, Copy)]
pub struct ListEndpoint {
    pub surface: &'static str,
    pub operation: &'static str,
    pub path: &'static str,
}

/// Each browsable asset implements this. Pure data mapping — no I/O, no state.
pub trait ResourceKind: Sync {
    fn columns(&self) -> &'static [Column];
    /// Pull the array of item objects out of a raw list-endpoint payload.
    fn extract_items(&self, payload: &Value) -> Vec<Value>;
    /// Map one item object to a display row (cells + stable id).
    fn row(&self, item: &Value) -> Row;
    fn list_endpoint(&self) -> ListEndpoint;
}

/// Registry: map a `Kind` to its static trait object. Filled per-asset.
pub fn kind_impl(kind: Kind) -> &'static dyn ResourceKind {
    match kind {
        Kind::Flow => &flow::FlowKind,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_name_and_all() {
        assert_eq!(Kind::Flow.name(), "flows");
        assert!(Kind::all().contains(&Kind::Flow));
    }

    #[test]
    fn cell_constructors_carry_tone() {
        let plain = Cell::plain("hello");
        assert_eq!(plain.text, "hello");
        assert_eq!(plain.tone, StatusTone::Neutral);

        let toned = Cell::toned("failed", StatusTone::Danger);
        assert_eq!(toned.tone, StatusTone::Danger);
    }
}
