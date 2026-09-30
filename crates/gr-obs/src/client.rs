//! Minimal obs-websocket v5 client (JSON over a local WebSocket).
//! Protocol: https://github.com/obsproject/obs-websocket/blob/master/docs/generated/protocol.md

use anyhow::{anyhow, bail, Context};
use base64::Engine as _;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;

/// Event subscriptions: General (1) | Outputs (64).
const EVENT_SUBS: u64 = 1 | 64;

#[derive(Debug, Clone)]
pub struct ObsEvent {
    pub event_type: String,
    pub data: Value,
}

type Pending = Arc<Mutex<HashMap<String, oneshot::Sender<Result<Value, String>>>>>;

pub struct ObsClient {
    out: mpsc::UnboundedSender<Message>,
    pending: Pending,
    next_id: AtomicU64,
    alive: Arc<AtomicBool>,
    events: broadcast::Sender<ObsEvent>,
}

pub fn auth_string(password: &str, salt: &str, challenge: &str) -> String {
    let b64 = base64::engine::general_purpose::STANDARD;
    let secret = b64.encode(Sha256::digest(format!("{password}{salt}").as_bytes()));
    b64.encode(Sha256::digest(format!("{secret}{challenge}").as_bytes()))
}

impl ObsClient {
    pub async fn connect(host: &str, port: u16, password: &str) -> anyhow::Result<ObsClient> {
        let url = format!("ws://{host}:{port}");
        let (ws, _) = tokio::time::timeout(Duration::from_secs(3), tokio_tungstenite::connect_async(url.as_str()))
            .await
            .map_err(|_| anyhow!("timed out connecting to OBS at {url}"))?
            .with_context(|| format!("OBS isn't reachable at {url} (is OBS running with the WebSocket server enabled?)"))?;
        let (mut sink, mut stream) = ws.split();

        // Hello -> Identify -> Identified
        let hello = read_json(&mut stream).await?;
        if hello["op"] != 0 {
            bail!("unexpected first message from OBS: {hello}");
        }
        let mut identify = json!({ "rpcVersion": 1, "eventSubscriptions": EVENT_SUBS });
        if let Some(auth) = hello["d"].get("authentication") {
            if password.is_empty() {
                bail!("OBS WebSocket needs a password. Enter it in Settings > OBS.");
            }
            let s = auth_string(password, auth["salt"].as_str().unwrap_or(""), auth["challenge"].as_str().unwrap_or(""));
            identify["authentication"] = json!(s);
        }
        sink.send(Message::Text(json!({ "op": 1, "d": identify }).to_string().into())).await?;
        let identified = read_json(&mut stream).await.map_err(|e| anyhow!("OBS rejected the connection (wrong WebSocket password?): {e}"))?;
        if identified["op"] != 2 {
            bail!("OBS rejected the connection (wrong WebSocket password?)");
        }

        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Message>();
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let alive = Arc::new(AtomicBool::new(true));
        let (events, _) = broadcast::channel(64);

        // Writer task.
        let alive_w = alive.clone();
        tokio::spawn(async move {
            while let Some(m) = out_rx.recv().await {
                if sink.send(m).await.is_err() {
                    break;
                }
            }
            alive_w.store(false, Ordering::SeqCst);
        });
        // Reader task.
        let (pending_r, alive_r, events_r, out_r) = (pending.clone(), alive.clone(), events.clone(), out_tx.clone());
        tokio::spawn(async move {
            while let Some(msg) = stream.next().await {
                let text = match msg {
                    Ok(Message::Text(t)) => t.to_string(),
                    Ok(Message::Ping(p)) => {
                        let _ = out_r.send(Message::Pong(p));
                        continue;
                    }
                    Ok(Message::Close(_)) | Err(_) => break,
                    _ => continue,
                };
                let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
                match v["op"].as_u64() {
                    Some(7) => {
                        let d = &v["d"];
                        let id = d["requestId"].as_str().unwrap_or_default().to_string();
                        if let Some(tx) = pending_r.lock().unwrap().remove(&id) {
                            let ok = d["requestStatus"]["result"].as_bool().unwrap_or(false);
                            let res = if ok {
                                Ok(d.get("responseData").cloned().unwrap_or(Value::Null))
                            } else {
                                let code = d["requestStatus"]["code"].as_i64().unwrap_or(0);
                                let comment = d["requestStatus"]["comment"].as_str().unwrap_or("").to_string();
                                Err(format!("{} failed (code {code}): {comment}", d["requestType"].as_str().unwrap_or("request")))
                            };
                            let _ = tx.send(res);
                        }
                    }
                    Some(5) => {
                        let d = &v["d"];
                        let _ = events_r.send(ObsEvent {
                            event_type: d["eventType"].as_str().unwrap_or_default().to_string(),
                            data: d.get("eventData").cloned().unwrap_or(Value::Null),
                        });
                    }
                    _ => {}
                }
            }
            alive_r.store(false, Ordering::SeqCst);
            // Fail everything still waiting.
            for (_, tx) in pending_r.lock().unwrap().drain() {
                let _ = tx.send(Err("connection to OBS closed".into()));
            }
        });

        Ok(ObsClient { out: out_tx, pending, next_id: AtomicU64::new(1), alive, events })
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ObsEvent> {
        self.events.subscribe()
    }

    /// Sends a request and waits for its response data.
    pub async fn request(&self, request_type: &str, data: Value) -> anyhow::Result<Value> {
        if !self.is_alive() {
            bail!("not connected to OBS");
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst).to_string();
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id.clone(), tx);
        let mut d = json!({ "requestType": request_type, "requestId": id });
        if !data.is_null() {
            d["requestData"] = data;
        }
        self.out.send(Message::Text(json!({ "op": 6, "d": d }).to_string().into())).map_err(|_| anyhow!("not connected to OBS"))?;
        match tokio::time::timeout(Duration::from_secs(20), rx).await {
            Ok(Ok(Ok(v))) => Ok(v),
            Ok(Ok(Err(e))) => Err(anyhow!(e)),
            Ok(Err(_)) => bail!("connection to OBS closed"),
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                bail!("{request_type}: OBS didn't answer in time")
            }
        }
    }

    pub async fn close(&self) {
        let _ = self.out.send(Message::Close(None));
    }
}

async fn read_json<S>(stream: &mut S) -> anyhow::Result<Value>
where
    S: futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .map_err(|_| anyhow!("OBS didn't respond"))?
            .ok_or_else(|| anyhow!("OBS closed the connection"))??;
        match msg {
            Message::Text(t) => return Ok(serde_json::from_str(&t)?),
            Message::Close(f) => bail!("OBS closed the connection{}", f.map(|f| format!(": {}", f.reason)).unwrap_or_default()),
            _ => continue,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn auth_matches_protocol_example() {
        // Example values from the obs-websocket protocol docs.
        let s = auth_string("supersecretpassword", "lM1GncleQOaCu9lT1yeUZhFYnqhsLLP1G5lAGo3ixaI=", "+IxH4CnCiqpX1rM9scsNynZzbOe4KhDeYcTNS3PDaeY=");
        assert_eq!(s, "1Ct943GAT+6YQUUX47Ia/ncufilbe6+oD6lY+5kaCu4=");
    }
}
