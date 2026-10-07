//! In-memory replay buffer: the last N seconds of encoded packets (nothing is re-encoded
//! to save a clip). Trimmed at keyframes so a clip always starts with a full picture.

use crate::mp4::Packet;
use std::collections::VecDeque;

pub struct ReplayBuffer {
    window: i64,
    max_bytes: usize,
    packets: VecDeque<Packet>,
    bytes: usize,
}

impl ReplayBuffer {
    /// `window_secs` of history, but never more than `max_bytes` of memory.
    pub fn new(window_secs: u32, max_bytes: usize) -> Self {
        Self { window: window_secs as i64 * crate::mp4::HNS, max_bytes, packets: VecDeque::new(), bytes: 0 }
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn push(&mut self, p: Packet) {
        let is_key = p.track == 0 && p.key;
        let now = p.pts;
        self.bytes += p.data.len();
        self.packets.push_back(p);
        if is_key {
            // Keep from the newest keyframe that is at least `window` old.
            let cutoff = now - self.window;
            if let Some(k) = self.packets.iter().filter(|q| q.track == 0 && q.key && q.pts <= cutoff).map(|q| q.pts).next_back() {
                self.drop_before(k);
            }
        }
        while self.bytes > self.max_bytes {
            // Drop up to the next keyframe after the first packet.
            let next_key = self.packets.iter().skip(1).find(|q| q.track == 0 && q.key).map(|q| q.pts);
            match next_key {
                Some(k) => self.drop_before(k),
                None => break,
            }
        }
    }

    fn drop_before(&mut self, pts: i64) {
        // Packets are roughly in time order across tracks; drop by timestamp.
        let mut kept = VecDeque::with_capacity(self.packets.len());
        for p in self.packets.drain(..) {
            if p.pts >= pts {
                kept.push_back(p);
            } else {
                self.bytes -= p.data.len();
            }
        }
        self.packets = kept;
    }

    /// Packets for a clip of the last `secs` seconds and the clip's start time.
    pub fn snapshot(&self, secs: u32) -> (Vec<Packet>, i64) {
        let latest = self.packets.iter().map(|p| p.pts).max().unwrap_or(0);
        let cutoff = latest - secs as i64 * crate::mp4::HNS;
        let keys: Vec<i64> = self.packets.iter().filter(|q| q.track == 0 && q.key).map(|q| q.pts).collect();
        let start = keys.iter().rev().find(|k| **k <= cutoff).or(keys.first()).copied().unwrap_or(0);
        let mut v: Vec<Packet> = self.packets.iter().filter(|p| p.pts >= start).cloned().collect();
        v.sort_by_key(|p| p.pts);
        (v, start)
    }

    pub fn clear(&mut self) {
        self.packets.clear();
        self.bytes = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const S: i64 = crate::mp4::HNS;

    fn v(t: f64, key: bool) -> Packet {
        Packet { track: 0, pts: (t * S as f64) as i64, data: vec![0; 100], key }
    }

    #[test]
    fn keeps_window_from_keyframe() {
        let mut r = ReplayBuffer::new(10, usize::MAX);
        for i in 0..300 {
            // 10 fps, keyframe every 2 s
            r.push(v(i as f64 / 10.0, i % 20 == 0));
        }
        let (clip, start) = r.snapshot(10);
        assert!(clip[0].key);
        assert_eq!(start, 18 * S); // latest 29.9 -> cutoff 19.9 -> keyframe at 18
                                   // Buffer never holds much more than the window + one GOP.
        let first = r.packets.front().unwrap().pts;
        assert!((16 * S..=20 * S).contains(&first), "first = {first}");
    }

    #[test]
    fn byte_cap() {
        let mut r = ReplayBuffer::new(1000, 5_000);
        for i in 0..300 {
            r.push(v(i as f64 / 10.0, i % 20 == 0));
        }
        assert!(r.bytes() <= 5_000 + 2_000);
        assert!(r.packets.front().unwrap().key);
    }
}
