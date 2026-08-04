use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::watch,
    task::JoinHandle,
    time::timeout,
};

pub const EICAR: &[u8] = b"X5O!P%@AP[4\\PZX54(P^)7CC)7}$EICAR-STANDARD-ANTIVIRUS-TEST-FILE!$H+H*";
pub const ERROR_TRIGGER: &[u8] = b"proofplane-test-clamd-error";

struct ClamdHangSignal {
    intercepted: watch::Sender<bool>,
    released: watch::Sender<bool>,
}

#[derive(Default)]
struct ClamdControlState {
    hangs: HashMap<Vec<u8>, Arc<ClamdHangSignal>>,
}

struct ClamdControlsInner {
    state: Mutex<ClamdControlState>,
}

#[derive(Clone)]
pub struct ClamdControls {
    inner: Arc<ClamdControlsInner>,
}

impl ClamdControls {
    fn new() -> Self {
        Self {
            inner: Arc::new(ClamdControlsInner {
                state: Mutex::new(ClamdControlState::default()),
            }),
        }
    }

    pub fn hang(&self) -> ClamdHangGuard {
        let mut document_bytes = vec![0_u8; 10];
        getrandom::fill(&mut document_bytes).expect("clamd hang bytes generate");
        let (intercepted, intercepted_receiver) = watch::channel(false);
        let (released, _) = watch::channel(false);
        let signal = Arc::new(ClamdHangSignal {
            intercepted,
            released,
        });

        self.inner
            .state
            .lock()
            .expect("clamd control state locks")
            .hangs
            .insert(document_bytes.clone(), signal.clone());

        ClamdHangGuard {
            controls: self.clone(),
            document_bytes,
            signal,
            intercepted_receiver,
            active: true,
        }
    }

    fn intercept(&self, document_bytes: &[u8]) -> Option<watch::Receiver<bool>> {
        let signal = self
            .inner
            .state
            .lock()
            .expect("clamd control state locks")
            .hangs
            .get(document_bytes)
            .cloned()?;

        signal.intercepted.send_replace(true);
        Some(signal.released.subscribe())
    }

    fn remove_hang(&self, document_bytes: &[u8]) {
        self.inner
            .state
            .lock()
            .expect("clamd control state locks")
            .hangs
            .remove(document_bytes);
    }
}

pub struct ClamdHangGuard {
    controls: ClamdControls,
    document_bytes: Vec<u8>,
    signal: Arc<ClamdHangSignal>,
    intercepted_receiver: watch::Receiver<bool>,
    active: bool,
}

impl ClamdHangGuard {
    pub fn document_bytes(&self) -> &[u8] {
        &self.document_bytes
    }

    pub async fn await_interception(&mut self) {
        timeout(Duration::from_secs(10), async {
            while !*self.intercepted_receiver.borrow() {
                self.intercepted_receiver
                    .changed()
                    .await
                    .expect("clamd interception source remains open");
            }
        })
        .await
        .expect("timed out after 10 seconds awaiting clamd interception");
    }

    pub fn release(mut self) {
        self.cleanup();
    }

    fn cleanup(&mut self) {
        if self.active {
            self.signal.released.send_replace(true);
            self.controls.remove_hang(&self.document_bytes);
            self.active = false;
        }
    }
}

impl Drop for ClamdHangGuard {
    fn drop(&mut self) {
        self.cleanup();
    }
}

pub struct FakeClamd {
    address: SocketAddr,
    controls: ClamdControls,
    // Retained so the listener remains owned by the suite harness.
    _task: JoinHandle<()>,
}

impl FakeClamd {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("fake clamd listener binds");
        let address = listener
            .local_addr()
            .expect("fake clamd listener has an address");
        let controls = ClamdControls::new();
        let server_controls = controls.clone();
        let task = tokio::spawn(async move {
            loop {
                let (stream, _) = listener
                    .accept()
                    .await
                    .expect("fake clamd accepts scanner connection");
                tokio::spawn(handle_connection(stream, server_controls.clone()));
            }
        });

        Self {
            address,
            controls,
            _task: task,
        }
    }

    pub fn address(&self) -> SocketAddr {
        self.address
    }

    pub fn controls(&self) -> ClamdControls {
        self.controls.clone()
    }
}

async fn handle_connection(mut stream: TcpStream, controls: ClamdControls) {
    let mut command = [0_u8; 10];
    stream
        .read_exact(&mut command)
        .await
        .expect("fake clamd reads INSTREAM command");
    assert_eq!(&command, b"zINSTREAM\0");

    let mut received = Vec::new();
    loop {
        let mut length = [0_u8; 4];
        stream
            .read_exact(&mut length)
            .await
            .expect("fake clamd reads INSTREAM frame length");
        let length = u32::from_be_bytes(length) as usize;
        if length == 0 {
            break;
        }

        let start = received.len();
        received.resize(start + length, 0);
        stream
            .read_exact(&mut received[start..])
            .await
            .expect("fake clamd reads INSTREAM frame");
    }

    let response = if let Some(mut released) = controls.intercept(&received) {
        while !*released.borrow() {
            if released.changed().await.is_err() {
                break;
            }
        }
        b"stream: OK\0".as_slice()
    } else if contains_bytes(&received, EICAR) {
        b"stream: Eicar-Test-Signature FOUND\0".as_slice()
    } else if contains_bytes(&received, ERROR_TRIGGER) {
        b"stream: injected scan failure ERROR\0".as_slice()
    } else {
        b"stream: OK\0".as_slice()
    };
    stream
        .write_all(response)
        .await
        .expect("fake clamd writes scan response");
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|candidate| candidate == needle)
}
