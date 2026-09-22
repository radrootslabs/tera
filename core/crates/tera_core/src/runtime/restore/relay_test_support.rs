use futures_util::{SinkExt, StreamExt};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};
use tokio_tungstenite::{accept_async, tungstenite::Message};

pub(super) struct Relay {
    pub url: String,
    pub event: Arc<Mutex<Option<serde_json::Value>>>,
    pub requests: Arc<Mutex<Vec<serde_json::Value>>>,
    pub sent: Arc<Mutex<Vec<serde_json::Value>>>,
    pub interrupt_query: Arc<AtomicBool>,
    stop: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

impl Relay {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let event = Arc::new(Mutex::new(None::<serde_json::Value>));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let sent = Arc::new(Mutex::new(Vec::new()));
        let interrupt_query = Arc::new(AtomicBool::new(false));
        let interrupted = interrupt_query.clone();
        let (stop, mut stopping) = oneshot::channel();
        let (events, queries, deliveries) = (event.clone(), requests.clone(), sent.clone());
        let task = tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    _ = &mut stopping => { connections.abort_all(); break; }
                    accepted = listener.accept() => {
                        let stream = accepted.unwrap().0;
                        let (events, queries, deliveries) = (events.clone(), queries.clone(), deliveries.clone());
                        let interrupted = interrupted.clone();
                        connections.spawn(async move {
                            let mut socket = accept_async(stream).await.unwrap();
                            while let Some(Ok(message)) = socket.next().await {
                                let Message::Text(text) = message else { continue; };
                                let value: serde_json::Value = serde_json::from_str(&text).unwrap();
                                let mut replies = Vec::new();
                                if value[0] == "REQ" {
                                    queries.lock().unwrap().push(value.clone());
                                    if interrupted.load(Ordering::SeqCst) {
                                        let _ = socket.close(None).await;
                                        return;
                                    }
                                    if let Some(event) = events.lock().unwrap().clone() {
                                        // Real duplicate wire observations must retain the original event ID.
                                        replies.push(serde_json::json!(["EVENT", value[1], event]));
                                        replies.push(serde_json::json!(["EVENT", value[1], event]));
                                    }
                                    replies.push(serde_json::json!(["EOSE", value[1]]));
                                } else if value[0] == "EVENT" {
                                    deliveries.lock().unwrap().push(value[1].clone());
                                    replies.push(serde_json::json!(["OK", value[1]["id"], true, ""]));
                                }
                                for reply in replies {
                                    if socket.send(Message::Text(reply.to_string().into())).await.is_err() { return; }
                                }
                            }
                        });
                    }
                }
            }
        });
        Self {
            url,
            event,
            requests,
            sent,
            interrupt_query,
            stop,
            task,
        }
    }

    pub async fn finish(self) {
        self.stop.send(()).unwrap();
        self.task.await.unwrap();
    }
}
