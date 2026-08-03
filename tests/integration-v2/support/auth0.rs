use std::{
    collections::HashMap,
    sync::{mpsc, OnceLock},
    sync::{Arc, Mutex},
    thread,
};

use async_trait::async_trait;
use axum::{extract::Form, routing::post, Json, Router};
use proofplane::{
    authentication::auth0::{
        AuditorIdentityExchange, AuditorIdentityProvider, AuditorIdentityProviderError,
        VerifiedAuditorIdentity,
    },
    domain::Sha256Digest,
};
use secrecy::ExposeSecret;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::net::TcpListener;
use url::Url;
use uuid::Uuid;

const BIND: &str = "127.0.0.1:9099";

#[derive(Clone, Default)]
pub struct FakeAuditorIdentityProvider {
    state: Arc<Mutex<AuditorIdentityState>>,
}

#[derive(Default)]
struct AuditorIdentityState {
    outcomes: HashMap<String, RegisteredOutcome>,
    exchanges: HashMap<String, Vec<RecordedAuditorIdentityExchange>>,
}

#[derive(Clone)]
struct RegisteredOutcome {
    registration_id: Uuid,
    outcome: AuditorIdentityOutcome,
}

#[derive(Clone)]
enum AuditorIdentityOutcome {
    Verified(VerifiedAuditorIdentity),
    Rejected,
    Unavailable,
}

#[derive(Clone, PartialEq, Eq)]
pub struct RecordedAuditorIdentityExchange {
    pub authorization_code: String,
    pub redirect_uri: Url,
    pub pkce_verifier: String,
    pub expected_nonce_digest: Sha256Digest,
}

impl FakeAuditorIdentityProvider {
    pub fn verified(
        &self,
        authorization_code: &str,
        subject: &str,
        email: &str,
    ) -> AuditorIdentityOutcomeGuard {
        self.register(
            authorization_code,
            AuditorIdentityOutcome::Verified(VerifiedAuditorIdentity {
                subject: subject.to_owned(),
                email: email.to_owned(),
                email_verified: true,
            }),
        )
    }

    pub fn unverified(
        &self,
        authorization_code: &str,
        subject: &str,
        email: &str,
    ) -> AuditorIdentityOutcomeGuard {
        self.register(
            authorization_code,
            AuditorIdentityOutcome::Verified(VerifiedAuditorIdentity {
                subject: subject.to_owned(),
                email: email.to_owned(),
                email_verified: false,
            }),
        )
    }

    pub fn rejected(&self, authorization_code: &str) -> AuditorIdentityOutcomeGuard {
        self.register(authorization_code, AuditorIdentityOutcome::Rejected)
    }

    pub fn unavailable(&self, authorization_code: &str) -> AuditorIdentityOutcomeGuard {
        self.register(authorization_code, AuditorIdentityOutcome::Unavailable)
    }

    pub fn exchanges_for(&self, authorization_code: &str) -> Vec<RecordedAuditorIdentityExchange> {
        self.state
            .lock()
            .expect("auditor identity provider state locks")
            .exchanges
            .get(authorization_code)
            .cloned()
            .unwrap_or_default()
    }

    fn register(
        &self,
        authorization_code: &str,
        outcome: AuditorIdentityOutcome,
    ) -> AuditorIdentityOutcomeGuard {
        let registration_id = Uuid::new_v4();
        self.state
            .lock()
            .expect("auditor identity provider state locks")
            .outcomes
            .insert(
                authorization_code.to_owned(),
                RegisteredOutcome {
                    registration_id,
                    outcome,
                },
            );
        AuditorIdentityOutcomeGuard {
            state: self.state.clone(),
            authorization_code: authorization_code.to_owned(),
            registration_id,
        }
    }
}

pub struct AuditorIdentityOutcomeGuard {
    state: Arc<Mutex<AuditorIdentityState>>,
    authorization_code: String,
    registration_id: Uuid,
}

impl Drop for AuditorIdentityOutcomeGuard {
    fn drop(&mut self) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state
            .outcomes
            .get(&self.authorization_code)
            .is_some_and(|registered| registered.registration_id == self.registration_id)
        {
            state.outcomes.remove(&self.authorization_code);
        }
    }
}

#[async_trait]
impl AuditorIdentityProvider for FakeAuditorIdentityProvider {
    async fn exchange_and_verify(
        &self,
        exchange: AuditorIdentityExchange,
    ) -> Result<VerifiedAuditorIdentity, AuditorIdentityProviderError> {
        let authorization_code = exchange.authorization_code.expose_secret().to_owned();
        let recorded = RecordedAuditorIdentityExchange {
            authorization_code: authorization_code.clone(),
            redirect_uri: exchange.redirect_uri,
            pkce_verifier: exchange.pkce_verifier.expose_secret().to_owned(),
            expected_nonce_digest: exchange.expected_nonce_digest,
        };
        let outcome = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| AuditorIdentityProviderError::Unavailable)?;
            state
                .exchanges
                .entry(authorization_code.clone())
                .or_default()
                .push(recorded);
            state
                .outcomes
                .get(authorization_code.as_str())
                .map(|registered| registered.outcome.clone())
        };

        match outcome {
            Some(AuditorIdentityOutcome::Verified(identity)) => Ok(identity),
            Some(AuditorIdentityOutcome::Unavailable) => {
                Err(AuditorIdentityProviderError::Unavailable)
            }
            Some(AuditorIdentityOutcome::Rejected) | None => {
                Err(AuditorIdentityProviderError::Rejected)
            }
        }
    }
}

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
