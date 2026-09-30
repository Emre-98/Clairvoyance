//! Sends raw obs-websocket requests: `obs_raw <password> <RequestType> [json] [RequestType json]...`
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let c = gr_obs::client::ObsClient::connect("127.0.0.1", 4455, &args[0]).await?;
    let mut ev = c.subscribe();
    let mut i = 1;
    while i < args.len() {
        let t = &args[i];
        let d: serde_json::Value = args.get(i + 1).filter(|s| s.starts_with('{')).map(|s| serde_json::from_str(s).unwrap()).unwrap_or(serde_json::Value::Null);
        i += if d.is_null() { 1 } else { 2 };
        println!("{t} -> {:?}", c.request(t, d).await);
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        while let Ok(e) = ev.try_recv() { println!("  event {} {}", e.event_type, e.data); }
    }
    Ok(())
}
