use std::{
    collections::{HashMap, VecDeque},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use axum::{body::Bytes, extract::State, http::StatusCode, routing::post, Router};
use axum_test::TestServer;
use metrics_exporter_prometheus::PrometheusBuilder;
use proofplane::{
    config::AppConfig,
    object_storage::FilesystemObjectStore,
    repository::Postgres,
    scanner::ClamAvMalwareScanner,
    worker::{create_worker_app, decode_worker_message, WorkerAppDependencies},
};
use tokio::{
    net::TcpListener,
    sync::{broadcast, watch},
    task::JoinHandle,
    time::{timeout, Instant},
};
use uuid::Uuid;

#[derive(Clone, Debug)]
struct PipelineEvent {
    event_type: String,
    aggregate_id: String,
    worker_status: StatusCode,
}

#[derive(Clone)]
pub struct PipelineEvents {
    sender: broadcast::Sender<PipelineEvent>,
}

impl PipelineEvents {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(256);
        Self { sender }
    }

    pub fn subscribe(&self) -> PipelineEventReceiver {
        PipelineEventReceiver {
            receiver: self.sender.subscribe(),
            pending: VecDeque::new(),
        }
    }
}

pub struct PipelineEventReceiver {
    receiver: broadcast::Receiver<PipelineEvent>,
    pending: VecDeque<PipelineEvent>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PipelineInterception {
    pub event_type: String,
    pub aggregate_id: String,
    pub message_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PipelineDeliveryObservation {
    pub event_type: String,
    pub aggregate_id: String,
    pub message_id: String,
    pub worker_status: StatusCode,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ReservationKey {
    event_type: String,
    request_id: Uuid,
}

struct HoldSignal {
    interception: watch::Sender<Option<PipelineInterception>>,
    released: watch::Sender<bool>,
}

struct FailureSignal {
    observations: watch::Sender<Vec<PipelineDeliveryObservation>>,
}

struct HoldEntry {
    token: u64,
    aggregate_id: Option<String>,
    signal: Arc<HoldSignal>,
}

struct FailureEntry {
    token: u64,
    aggregate_id: Option<String>,
    message_id: Option<String>,
    claimed_deliveries: usize,
    signal: Arc<FailureSignal>,
}

#[derive(Default)]
struct PipelineControlState {
    holds: HashMap<ReservationKey, HoldEntry>,
    failures: HashMap<ReservationKey, FailureEntry>,
}

struct PipelineControlsInner {
    state: Mutex<PipelineControlState>,
    next_token: AtomicU64,
}

#[derive(Clone)]
pub struct PipelineControls {
    inner: Arc<PipelineControlsInner>,
}

impl PipelineControls {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(PipelineControlsInner {
                state: Mutex::new(PipelineControlState::default()),
                next_token: AtomicU64::new(1),
            }),
        }
    }

    pub fn hold(&self, event_type: &str, request_id: Uuid) -> PipelineGate {
        let key = ReservationKey {
            event_type: event_type.to_owned(),
            request_id,
        };
        let token = self.inner.next_token.fetch_add(1, Ordering::Relaxed);
        let (interception, interception_receiver) = watch::channel(None);
        let (released, _) = watch::channel(false);
        let signal = Arc::new(HoldSignal {
            interception,
            released,
        });

        let replaced = self
            .inner
            .state
            .lock()
            .expect("pipeline control state locks")
            .holds
            .insert(
                key.clone(),
                HoldEntry {
                    token,
                    aggregate_id: None,
                    signal: signal.clone(),
                },
            );
        if let Some(replaced) = replaced {
            replaced.signal.released.send_replace(true);
        }

        PipelineGate {
            controls: self.clone(),
            key,
            token,
            signal,
            interception_receiver,
            active: true,
        }
    }

    pub fn fail_after_forward_once(
        &self,
        event_type: &str,
        request_id: Uuid,
    ) -> PipelineFailureGuard {
        let key = ReservationKey {
            event_type: event_type.to_owned(),
            request_id,
        };
        let token = self.inner.next_token.fetch_add(1, Ordering::Relaxed);
        let (observations, observations_receiver) = watch::channel(Vec::new());
        let signal = Arc::new(FailureSignal { observations });

        self.inner
            .state
            .lock()
            .expect("pipeline control state locks")
            .failures
            .insert(
                key.clone(),
                FailureEntry {
                    token,
                    aggregate_id: None,
                    message_id: None,
                    claimed_deliveries: 0,
                    signal: signal.clone(),
                },
            );

        PipelineFailureGuard {
            controls: self.clone(),
            key,
            token,
            observations_receiver,
            active: true,
        }
    }

    fn directive(&self, message: &proofplane::worker::WorkerMessage) -> ForwardDirective {
        let Some(request_id) = message.request_id else {
            return ForwardDirective::Immediate;
        };
        let key = ReservationKey {
            event_type: message.event_type.clone(),
            request_id,
        };
        let mut state = self
            .inner
            .state
            .lock()
            .expect("pipeline control state locks");

        if let Some(entry) = state.holds.get_mut(&key) {
            if entry
                .aggregate_id
                .as_ref()
                .is_none_or(|aggregate_id| aggregate_id == &message.aggregate_id)
            {
                entry.aggregate_id = Some(message.aggregate_id.clone());
                let interception = PipelineInterception {
                    event_type: message.event_type.clone(),
                    aggregate_id: message.aggregate_id.clone(),
                    message_id: message.message_id.clone(),
                };
                entry.signal.interception.send_replace(Some(interception));
                return ForwardDirective::Hold(entry.signal.released.subscribe());
            }
        }

        if let Some(entry) = state.failures.get_mut(&key) {
            let aggregate_matches = entry
                .aggregate_id
                .as_ref()
                .is_none_or(|aggregate_id| aggregate_id == &message.aggregate_id);
            let message_matches = entry
                .message_id
                .as_ref()
                .is_none_or(|message_id| message_id == &message.message_id);
            if aggregate_matches && message_matches && entry.claimed_deliveries < 2 {
                entry.aggregate_id = Some(message.aggregate_id.clone());
                entry.message_id = Some(message.message_id.clone());
                let delivery_index = entry.claimed_deliveries;
                entry.claimed_deliveries += 1;
                return ForwardDirective::ObserveFailureDelivery {
                    signal: entry.signal.clone(),
                    fail_response: delivery_index == 0,
                };
            }
        }

        ForwardDirective::Immediate
    }

    fn remove_hold(&self, key: &ReservationKey, token: u64) {
        let mut state = self
            .inner
            .state
            .lock()
            .expect("pipeline control state locks");
        if state
            .holds
            .get(key)
            .is_some_and(|entry| entry.token == token)
        {
            state.holds.remove(key);
        }
    }

    fn remove_failure(&self, key: &ReservationKey, token: u64) {
        let mut state = self
            .inner
            .state
            .lock()
            .expect("pipeline control state locks");
        if state
            .failures
            .get(key)
            .is_some_and(|entry| entry.token == token)
        {
            state.failures.remove(key);
        }
    }
}

pub struct PipelineGate {
    controls: PipelineControls,
    key: ReservationKey,
    token: u64,
    signal: Arc<HoldSignal>,
    interception_receiver: watch::Receiver<Option<PipelineInterception>>,
    active: bool,
}

impl PipelineGate {
    pub async fn await_interception(&mut self) -> PipelineInterception {
        let awaited = timeout(Duration::from_secs(10), async {
            loop {
                if let Some(interception) = self.interception_receiver.borrow().clone() {
                    return interception;
                }
                self.interception_receiver
                    .changed()
                    .await
                    .expect("pipeline hold interception source remains open");
            }
        })
        .await;
        awaited.expect("timed out after 10 seconds awaiting pipeline interception")
    }

    pub fn release(mut self) {
        self.cleanup();
    }

    fn cleanup(&mut self) {
        if self.active {
            self.signal.released.send_replace(true);
            self.controls.remove_hold(&self.key, self.token);
            self.active = false;
        }
    }
}

impl Drop for PipelineGate {
    fn drop(&mut self) {
        self.cleanup();
    }
}

pub struct PipelineFailureGuard {
    controls: PipelineControls,
    key: ReservationKey,
    token: u64,
    observations_receiver: watch::Receiver<Vec<PipelineDeliveryObservation>>,
    active: bool,
}

impl PipelineFailureGuard {
    pub async fn await_first_delivery(&mut self) -> PipelineDeliveryObservation {
        self.await_deliveries(1).await.remove(0)
    }

    pub async fn await_redelivery(&mut self) -> [PipelineDeliveryObservation; 2] {
        self.await_deliveries(2)
            .await
            .try_into()
            .expect("exactly two pipeline deliveries are returned")
    }

    pub fn release(mut self) {
        self.cleanup();
    }

    async fn await_deliveries(&mut self, count: usize) -> Vec<PipelineDeliveryObservation> {
        let awaited = timeout(Duration::from_secs(10), async {
            loop {
                let observations = self.observations_receiver.borrow().clone();
                if observations.len() >= count {
                    return observations.into_iter().take(count).collect();
                }
                self.observations_receiver
                    .changed()
                    .await
                    .expect("pipeline failure observation source remains open");
            }
        })
        .await;
        awaited.expect("timed out after 10 seconds awaiting pipeline redelivery")
    }

    fn cleanup(&mut self) {
        if self.active {
            self.controls.remove_failure(&self.key, self.token);
            self.active = false;
        }
    }
}

impl Drop for PipelineFailureGuard {
    fn drop(&mut self) {
        self.cleanup();
    }
}

enum ForwardDirective {
    Immediate,
    Hold(watch::Receiver<bool>),
    ObserveFailureDelivery {
        signal: Arc<FailureSignal>,
        fail_response: bool,
    },
}

impl PipelineEventReceiver {
    pub async fn await_event(&mut self, event_type: &str, aggregate_id: &str) -> StatusCode {
        if let Some(index) = self
            .pending
            .iter()
            .position(|event| event.event_type == event_type && event.aggregate_id == aggregate_id)
        {
            return self
                .pending
                .remove(index)
                .expect("located pending pipeline event exists")
                .worker_status;
        }

        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let received = timeout(remaining, self.receiver.recv()).await;
            match received {
                Ok(Ok(event))
                    if event.event_type == event_type && event.aggregate_id == aggregate_id =>
                {
                    return event.worker_status;
                }
                Ok(Ok(event)) if event.aggregate_id == aggregate_id => {
                    // Deltio does not promise ordering. Keep a later event for this
                    // aggregate so a subsequent await cannot miss it.
                    self.pending.push_back(event);
                }
                Ok(Ok(_)) => {}
                Ok(Err(broadcast::error::RecvError::Closed)) => {
                    panic!(
                        "pipeline event source closed while awaiting {event_type} for {aggregate_id}"
                    );
                }
                Ok(Err(broadcast::error::RecvError::Lagged(skipped))) => {
                    panic!(
                        "pipeline event receiver lagged by {skipped} messages while awaiting \
                         {event_type} for {aggregate_id}"
                    );
                }
                Err(_) => {
                    panic!(
                        "timed out after 10 seconds awaiting pipeline event {event_type} for \
                         {aggregate_id}"
                    );
                }
            }
        }
    }
}

pub struct PushProxy {
    port: u16,
    // Retained so the proxy remains owned by the suite harness.
    _task: JoinHandle<()>,
}

#[derive(Clone)]
struct ProxyState {
    client: reqwest::Client,
    worker_endpoint: String,
    events: PipelineEvents,
    controls: PipelineControls,
}

impl PushProxy {
    pub async fn start(
        worker: &TestServer,
        events: PipelineEvents,
        controls: PipelineControls,
    ) -> Self {
        let worker_endpoint = worker
            .server_url("/pubsub/messages")
            .expect("worker push endpoint builds")
            .to_string();
        let listener = TcpListener::bind("0.0.0.0:0")
            .await
            .expect("pipeline push proxy binds");
        let port = listener
            .local_addr()
            .expect("pipeline push proxy has an address")
            .port();
        let state = ProxyState {
            client: reqwest::Client::new(),
            worker_endpoint,
            events,
            controls,
        };
        let app = Router::new()
            .route("/pubsub/messages", post(forward_message))
            .with_state(state);
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("pipeline push proxy serves");
        });

        Self { port, _task: task }
    }

    pub fn container_endpoint(&self) -> url::Url {
        url::Url::parse(&format!(
            "http://host.docker.internal:{}/pubsub/messages",
            self.port
        ))
        .expect("container-visible proxy endpoint parses")
    }
}

async fn forward_message(State(state): State<ProxyState>, body: Bytes) -> StatusCode {
    let identity = decode_worker_message(&body).ok();
    let mut directive = identity
        .as_ref()
        .map(|message| state.controls.directive(message))
        .unwrap_or(ForwardDirective::Immediate);
    if let ForwardDirective::Hold(released) = &mut directive {
        while !*released.borrow() {
            if released.changed().await.is_err() {
                break;
            }
        }
    }
    let response = match state
        .client
        .post(&state.worker_endpoint)
        .body(body)
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            tracing::error!(%error, "pipeline push proxy failed to reach worker");
            return StatusCode::BAD_GATEWAY;
        }
    };
    let worker_status =
        StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);

    if let Some(message) = identity {
        let observation = PipelineDeliveryObservation {
            event_type: message.event_type.clone(),
            aggregate_id: message.aggregate_id.clone(),
            message_id: message.message_id.clone(),
            worker_status,
        };
        if let ForwardDirective::ObserveFailureDelivery {
            signal,
            fail_response,
        } = directive
        {
            signal.observations.send_modify(|observations| {
                if observations.len() < 2 {
                    observations.push(observation);
                }
            });
            let _ = state.events.sender.send(PipelineEvent {
                event_type: message.event_type,
                aggregate_id: message.aggregate_id,
                worker_status,
            });
            return if fail_response {
                StatusCode::INTERNAL_SERVER_ERROR
            } else {
                worker_status
            };
        }
        let _ = state.events.sender.send(PipelineEvent {
            event_type: message.event_type,
            aggregate_id: message.aggregate_id,
            worker_status,
        });
    }

    worker_status
}

pub fn start_worker(
    config: &AppConfig,
    postgres: &Arc<Postgres>,
    object_store: &Arc<FilesystemObjectStore>,
) -> TestServer {
    let recorder = PrometheusBuilder::new().build_recorder();
    let scanner = Arc::new(ClamAvMalwareScanner::new(
        config.scanner.clamd_address,
        Duration::from_millis(config.scanner.connection_timeout_ms),
        Duration::from_millis(config.scanner.scan_timeout_ms),
    ));

    TestServer::builder()
        .http_transport()
        .build(create_worker_app(WorkerAppDependencies {
            postgres: postgres.clone(),
            object_store: object_store.clone(),
            scanner,
            worker_max_delivery_attempts: config.pubsub.subscriptions.worker_max_delivery_attempts,
            metrics: recorder.handle(),
            live_path: config.health.live_path.clone(),
            ready_path: config.health.ready_path.clone(),
            dependency_timeout_ms: config.health.dependency_timeout_ms,
        }))
}
