pub mod failures;
pub mod jobs;
pub mod overview;
pub mod workflows;

use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use maud::Markup;

/// Wrap a `Markup` into a 200 HTML response.
pub fn html(m: Markup) -> Response {
    Html(m.into_string()).into_response()
}

/// Render a handler error as a small in-page card so the rest of the
/// dashboard keeps rendering even when one panel fails.
pub fn err_card(message: String) -> Response {
    (
        StatusCode::OK,
        Html(super::views::error_card(&message).into_string()),
    )
        .into_response()
}
