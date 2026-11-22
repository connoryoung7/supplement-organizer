//! HTTP: server-rendered pages (Askama) and plain form posts that redirect back.

mod actions;
mod format;
mod pages;

use askama::Template;
use axum::extract::Path;
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;
use tower_http::trace::TraceLayer;

use crate::app::{AppError, AppResult, Store};

pub fn router(store: Store) -> Router {
    Router::new()
        .route("/", get(pages::today))
        .route(
            "/products",
            get(pages::products).post(actions::create_product),
        )
        .route("/products/new", get(pages::new_product))
        .route(
            "/products/{id}",
            get(pages::product).post(actions::update_product),
        )
        .route("/products/{id}/edit", get(pages::edit_product))
        .route("/products/{id}/delete", post(actions::delete_product))
        .route("/products/{id}/status", post(actions::set_status))
        .route("/products/{id}/doses", post(actions::log_dose))
        .route("/products/{id}/bottles", post(actions::add_bottles))
        .route("/products/{id}/ingredients", post(actions::add_ingredient))
        .route(
            "/products/{id}/ingredients/{ingredient_id}/delete",
            post(actions::remove_ingredient),
        )
        .route("/bottles/{id}", post(actions::update_bottle))
        .route("/bottles/{id}/edit", get(pages::edit_bottle))
        .route("/bottles/{id}/delete", post(actions::delete_bottle))
        .route("/bottles/{id}/start", post(actions::start_bottle))
        .route("/bottles/{id}/count", post(actions::correct_count))
        .route(
            "/bottle-events/{id}/delete",
            post(actions::delete_bottle_event),
        )
        .route("/doses/{id}", post(actions::update_dose))
        .route("/doses/{id}/edit", get(pages::edit_dose))
        .route("/doses/{id}/delete", post(actions::delete_dose))
        .route("/at-risk", get(pages::at_risk))
        .route("/ingredients", get(pages::ingredients))
        .route("/ingredients/{id}/delete", post(actions::delete_ingredient))
        .route("/static/{file}", get(static_file))
        .route("/health", get(health))
        .fallback(|| async { AppError::NotFound })
        .layer(TraceLayer::new_for_http())
        .with_state(store)
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION") }))
}

/// Shared by every page: which tab is active, a flash error, and the at-risk badge.
pub struct Layout {
    pub active: &'static str,
    pub error: Option<String>,
    pub at_risk: usize,
}

fn render(page: &impl Template) -> AppResult<Response> {
    let html = page.render().map_err(|e| AppError::Internal(e.into()))?;
    Ok(Html(html).into_response())
}

#[derive(Template)]
#[template(path = "message.html")]
struct MessagePage {
    layout: Layout,
    title: String,
    message: String,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, title, message) = match self {
            AppError::NotFound => (
                StatusCode::NOT_FOUND,
                "Not found",
                "That page doesn't exist.".to_string(),
            ),
            AppError::Rule(msg) => (StatusCode::UNPROCESSABLE_ENTITY, "That didn't work", msg),
            AppError::Internal(e) => {
                tracing::error!("{e:#}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Something went wrong",
                    "The error has been logged.".to_string(),
                )
            }
        };
        let page = MessagePage {
            layout: Layout {
                active: "",
                error: None,
                at_risk: 0,
            },
            title: title.into(),
            message,
        };
        match page.render() {
            Ok(html) => (status, Html(html)).into_response(),
            Err(_) => (status, title).into_response(),
        }
    }
}

/// Only same-site paths are allowed as redirect targets.
fn safe_back(back: Option<&str>, default: &str) -> String {
    match back {
        Some(b) if b.starts_with('/') && !b.starts_with("//") => b.to_string(),
        _ => default.to_string(),
    }
}

fn with_error(url: &str, msg: &str) -> String {
    let base = url.split('?').next().unwrap_or(url);
    let query = serde_urlencoded::to_string([("error", msg)]).unwrap_or_default();
    format!("{base}?{query}")
}

/// After a form post: redirect to `ok` on success, or back to `err` with the rule that
/// was broken shown as a flash message.
fn respond<T>(
    result: AppResult<T>,
    ok: impl FnOnce(T) -> String,
    err: &str,
) -> AppResult<Response> {
    match result {
        Ok(value) => Ok(Redirect::to(&ok(value)).into_response()),
        Err(AppError::Rule(msg)) => Ok(Redirect::to(&with_error(err, &msg)).into_response()),
        Err(e) => Err(e),
    }
}

async fn static_file(Path(file): Path<String>) -> Response {
    let (body, content_type): (&'static [u8], &str) = match file.as_str() {
        "app.css" => (
            include_bytes!("../../static/app.css"),
            "text/css; charset=utf-8",
        ),
        "htmx.min.js" => (
            include_bytes!("../../static/htmx.min.js"),
            "text/javascript; charset=utf-8",
        ),
        "manifest.webmanifest" => (
            include_bytes!("../../static/manifest.webmanifest"),
            "application/manifest+json",
        ),
        "icon.svg" => (include_bytes!("../../static/icon.svg"), "image/svg+xml"),
        "icon-192.png" => (include_bytes!("../../static/icon-192.png"), "image/png"),
        "icon-512.png" => (include_bytes!("../../static/icon-512.png"), "image/png"),
        "apple-touch-icon.png" => (
            include_bytes!("../../static/apple-touch-icon.png"),
            "image/png",
        ),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "public, max-age=300"),
        ],
        body,
    )
        .into_response()
}
