//! Standalone fake League API: `mock-league [speed] [length_secs]`.
//! Prints the base URL; stays up until the scripted match is over.

fn main() {
    let mut args = std::env::args().skip(1);
    let mut o = cv_mock_league::MockOptions::default();
    if let Some(s) = args.next().and_then(|s| s.parse().ok()) {
        o.speed = s;
    }
    if let Some(l) = args.next().and_then(|s| s.parse().ok()) {
        o.length = l;
    }
    let h = cv_mock_league::spawn(o.clone()).expect("port in use?");
    println!("Fake League API at {} (speed x{}, {} s match)", h.base_url, o.speed, o.length);
    while h.running.load(std::sync::atomic::Ordering::SeqCst) {
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    println!("Match over.");
}
