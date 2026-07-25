use std::{
    sync::{mpsc, OnceLock},
    thread,
};

use axum::{extract::Form, routing::post, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::net::TcpListener;

const BIND: &str = "127.0.0.1:9099";

// TODO: Make the auth0 dependency generic so that fake implementations
// can be passed in for tests so that we don't need to do this.
// This fake auth0 server runs as a global process for all tests.
static STARTED: OnceLock<()> = OnceLock::new();

pub fn start() {
    STARTED.get_or_init(|| {
        let (ready, started) = mpsc::channel();

        thread::Builder::new()
            .name("fake-auth0".to_owned())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("fake Auth0 runtime builds");

                runtime.block_on(async move {
                    let listener = match TcpListener::bind(BIND).await {
                        Ok(listener) => listener,
                        Err(error) => {
                            let _ = ready.send(Err(format!(
                                "fake Auth0 could not bind {BIND} ({error}); \
                                 another `cargo test` run may already hold it"
                            )));
                            return;
                        }
                    };
                    let _ = ready.send(Ok(()));

                    axum::serve(listener, router())
                        .await
                        .expect("fake Auth0 serves");
                });
            })
            .expect("fake Auth0 thread spawns");

        match started.recv() {
            Ok(Ok(())) => {}
            Ok(Err(message)) => panic!("{message}"),
            Err(_) => panic!("fake Auth0 thread stopped before it was ready"),
        }
    });
}

fn router() -> Router {
    Router::new().route("/oauth/token", post(token))
}

/// The upstream exchange sends other fields, but only the `code`
/// decides which identity comes back.
#[derive(Deserialize)]
struct TokenRequest {
    code: String,
}

async fn token(Form(request): Form<TokenRequest>) -> Json<Value> {
    Json(json!({
        "access_token": request.code,
        "token_type": "Bearer",
        "expires_in": 86_400,
    }))
}
