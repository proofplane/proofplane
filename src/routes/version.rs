use axum::{routing::get, Json, Router};
use serde::Serialize;

use crate::{package_name, VERSION};

#[derive(Debug, Serialize)]
struct VersionResponse {
    package: &'static str,
    version: &'static str,
}

pub fn router() -> Router {
    Router::new().route("/", get(version))
}

async fn version() -> Json<VersionResponse> {
    Json(VersionResponse {
        package: package_name(),
        version: VERSION,
    })
}
