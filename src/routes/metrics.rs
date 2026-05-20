use axum::{extract::State, response::IntoResponse, routing::get, Router};
use metrics_exporter_prometheus::PrometheusHandle;

#[derive(Clone)]
pub struct MetricsState {
    pub handle: PrometheusHandle,
}

pub fn router(state: MetricsState) -> Router {
    Router::new().route("/", get(metrics)).with_state(state)
}

async fn metrics(State(state): State<MetricsState>) -> impl IntoResponse {
    (
        [("content-type", "text/plain; version=0.0.4")],
        state.handle.render(),
    )
}
