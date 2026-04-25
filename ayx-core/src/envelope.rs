use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Serialize)]
pub struct Envelope {
    pub ok: bool,
    pub message: String,
    pub timestamp_utc: DateTime<Utc>,
    pub data: Value,
}

impl Envelope {
    pub fn ok(message: impl Into<String>) -> Self {
        Self {
            ok: true,
            message: message.into(),
            timestamp_utc: Utc::now(),
            data: Value::Null,
        }
    }

    pub fn ok_with_data(message: impl Into<String>, data: Value) -> Self {
        Self {
            ok: true,
            message: message.into(),
            timestamp_utc: Utc::now(),
            data,
        }
    }

    pub fn err_with_data(message: impl Into<String>, data: Value) -> Self {
        Self {
            ok: false,
            message: message.into(),
            timestamp_utc: Utc::now(),
            data,
        }
    }
}
