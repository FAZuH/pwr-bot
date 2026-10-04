use std::collections::HashMap;
use std::io::Stdout;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::mpsc;

use pwr_plugin_protocol::Msg;
use pwr_plugin_protocol::WireError;
use pwr_plugin_support::write_msg;
use serde_json::Value;
use tokio::sync::oneshot;

#[derive(Clone, Debug, thiserror::Error)]
pub enum HostCallError {
    #[error("{kind}: {msg}")]
    Wire { kind: String, msg: String },
    #[error("host call channel closed")]
    ChannelClosed,
}

impl From<WireError> for HostCallError {
    fn from(value: WireError) -> Self {
        Self::Wire {
            kind: value.kind,
            msg: value.msg,
        }
    }
}

#[async_trait::async_trait]
pub trait HostClient: Send + Sync {
    async fn call(&self, op: &str, args: Value) -> Result<Value, HostCallError>;
}

pub type SharedOutput = Arc<Mutex<Stdout>>;

pub struct HostResponse {
    pub id: u64,
    pub ok: bool,
    pub data: Option<Value>,
    pub error: Option<WireError>,
}

enum BrokerEvent {
    Call {
        op: String,
        args: Value,
        respond: oneshot::Sender<Result<Value, HostCallError>>,
    },
    Response(HostResponse),
}

#[derive(Clone)]
pub struct HostCallClient {
    events: mpsc::Sender<BrokerEvent>,
}

impl HostCallClient {
    pub fn send_response(&self, response: HostResponse) {
        let _ = self.events.send(BrokerEvent::Response(response));
    }
}

#[async_trait::async_trait]
impl HostClient for HostCallClient {
    async fn call(&self, op: &str, args: Value) -> Result<Value, HostCallError> {
        let (respond, result) = oneshot::channel();
        self.events
            .send(BrokerEvent::Call {
                op: op.to_string(),
                args,
                respond,
            })
            .map_err(|_| HostCallError::ChannelClosed)?;
        result.await.map_err(|_| HostCallError::ChannelClosed)?
    }
}

pub fn spawn_host_client(output: SharedOutput, next_call_id: Arc<AtomicU64>) -> HostCallClient {
    let (events, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut pending = HashMap::new();
        while let Ok(event) = receiver.recv() {
            match event {
                BrokerEvent::Call { op, args, respond } => {
                    let id = next_call_id.fetch_add(1, Ordering::Relaxed);
                    let message = Msg::Call {
                        id,
                        op,
                        cmd: None,
                        args: Some(args),
                    };
                    let written = {
                        let mut output = output.lock().expect("stdout lock poisoned");
                        write_msg(&mut *output, &message)
                    };
                    if written.is_ok() {
                        pending.insert(id, respond);
                    } else {
                        let _ = respond.send(Err(HostCallError::ChannelClosed));
                    }
                }
                BrokerEvent::Response(HostResponse {
                    id,
                    ok,
                    data,
                    error,
                }) => {
                    let Some(respond) = pending.remove(&id) else {
                        continue;
                    };
                    let result = if ok {
                        Ok(data.unwrap_or(Value::Null))
                    } else {
                        Err(HostCallError::from(error.unwrap_or(WireError {
                            kind: "HostError".into(),
                            msg: "host call failed without an error".into(),
                        })))
                    };
                    let _ = respond.send(result);
                }
            }
        }
    });
    HostCallClient { events }
}
