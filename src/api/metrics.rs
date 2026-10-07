//! Prometheus text exposition for `/metrics`.
//!
//! Combines the runtime counters in [`crate::infra::metrics::Metrics`] with
//! live gauges (tokens, in-flight requests) and all-time token totals pulled
//! from `request_usage`, so scrapers see the same numbers as the dashboard.

use crate::api::state::AppState;
use crate::infra::db::get_tokens;
use crate::infra::usage_db::{get_all_summaries, get_model_breakdown};
use axum::{
    body::Body,
    extract::State,
    http::{header::CONTENT_TYPE, StatusCode},
    response::Response,
};
use std::fmt::Write;

const EXPOSITION_CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";

/// Scrape endpoint. Always returns 200 with the current metric family set.
pub async fn get_metrics(State(state): State<AppState>) -> Response {
    let tokens = get_tokens(&state.db).await.unwrap_or_default();
    let tokens_total = tokens.len() as u64;
    let tokens_active = tokens.iter().filter(|t| t.is_active()).count() as u64;
    let in_flight: u64 = state.in_flight.lock().await.values().sum::<usize>() as u64;
    let snap = state.metrics.snapshot();

    let all_time = get_all_summaries(&state.db)
        .await
        .unwrap_or_default()
        .into_iter()
        .find(|s| s.period == "All Time");
    let models = get_model_breakdown(&state.db).await.unwrap_or_default();

    let mut out = String::with_capacity(1024);

    write_header(
        &mut out,
        "deeperseeker_up",
        "Whether the service is up.",
        "gauge",
    );
    let _ = writeln!(out, "deeperseeker_up 1");

    write_header(
        &mut out,
        "deeperseeker_build_info",
        "Build information.",
        "gauge",
    );
    let _ = writeln!(
        out,
        "deeperseeker_build_info{{version=\"{}\"}} 1",
        env!("CARGO_PKG_VERSION")
    );

    write_gauge(
        &mut out,
        "deeperseeker_tokens_total",
        "Configured upstream tokens.",
        tokens_total,
    );
    write_gauge(
        &mut out,
        "deeperseeker_tokens_active",
        "Active upstream tokens.",
        tokens_active,
    );
    write_gauge(
        &mut out,
        "deeperseeker_in_flight_requests",
        "Requests currently in flight.",
        in_flight,
    );
    write_counter(
        &mut out,
        "deeperseeker_requests_total",
        "Chat requests processed since start.",
        snap.requests_total(),
    );
    write_counter(
        &mut out,
        "deeperseeker_requests_success_total",
        "Successful chat requests.",
        snap.requests_success,
    );
    write_counter(
        &mut out,
        "deeperseeker_requests_error_total",
        "Chat requests failed fatally.",
        snap.requests_error,
    );
    write_counter(
        &mut out,
        "deeperseeker_requests_rate_limited_total",
        "Chat requests rejected with HTTP 429.",
        snap.requests_rate_limited,
    );

    if let Some(s) = all_time {
        write_counter(
            &mut out,
            "deeperseeker_usage_requests_total",
            "All-time recorded usage rows.",
            s.requests,
        );
        write_counter(
            &mut out,
            "deeperseeker_prompt_tokens_total",
            "All-time prompt tokens.",
            s.prompt_tokens,
        );
        write_counter(
            &mut out,
            "deeperseeker_completion_tokens_total",
            "All-time completion tokens.",
            s.completion_tokens,
        );
        write_counter(
            &mut out,
            "deeperseeker_cached_tokens_total",
            "All-time cached tokens.",
            s.cached_tokens,
        );
    }

    write_header(
        &mut out,
        "deeperseeker_model_tokens_total",
        "All-time total tokens per model.",
        "counter",
    );
    for m in &models {
        let _ = writeln!(
            out,
            "deeperseeker_model_tokens_total{{model=\"{}\"}} {}",
            escape_label(&m.model),
            m.total_tokens
        );
    }

    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, EXPOSITION_CONTENT_TYPE)
        .body(Body::from(out))
        .unwrap_or_default()
}

fn write_header(out: &mut String, name: &str, help: &str, kind: &str) {
    let _ = writeln!(out, "# HELP {name} {help}");
    let _ = writeln!(out, "# TYPE {name} {kind}");
}

fn write_gauge(out: &mut String, name: &str, help: &str, value: u64) {
    write_header(out, name, help, "gauge");
    let _ = writeln!(out, "{name} {value}");
}

fn write_counter(out: &mut String, name: &str, help: &str, value: u64) {
    write_header(out, name, help, "counter");
    let _ = writeln!(out, "{name} {value}");
}

/// Escape a Prometheus label value per the text exposition format.
fn escape_label(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}
