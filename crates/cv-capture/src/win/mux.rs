//! Writer thread: turns encoder output into the crash-safe recording and keeps the replay buffer.

use super::video_enc::EncodedFrame;
use crate::mp4::{annexb_to_avcc, write_clip, AudioConfig, FragmentedWriter, Packet, VideoConfig};
use crate::replay::ReplayBuffer;
use anyhow::{anyhow, Result};
use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

pub enum MuxMsg {
    Video(EncodedFrame),
    Packet(Packet),
    SaveReplay { path: PathBuf, secs: u32, reply: Sender<Result<PathBuf>> },
    Stop { reply: Sender<Result<(PathBuf, f64)>> },
}

pub struct MuxParams {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub audio: Vec<AudioConfig>,
    pub replay_secs: u32,
    pub replay_max_bytes: usize,
    /// false = "clips only": keep the in-memory replay buffer (hotkey and event clips) but
    /// don't write the full recording to disk.
    pub full_video: bool,
}

pub fn run(p: MuxParams, rx: Receiver<MuxMsg>) {
    let mut writer: Option<FragmentedWriter<BufWriter<File>>> = None;
    let mut vcfg: Option<VideoConfig> = None;
    let (mut sps, mut pps): (Option<Vec<u8>>, Option<Vec<u8>>) = (None, None);
    let mut pending_audio: Vec<Packet> = Vec::new();
    let mut replay = ReplayBuffer::new(p.replay_secs.max(5), p.replay_max_bytes);
    let mut last_flush = Instant::now();
    let mut last_sync = Instant::now();
    let mut error: Option<String> = None;

    loop {
        let msg = match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(m) => Some(m),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        match msg {
            Some(MuxMsg::Video(f)) => {
                let au = annexb_to_avcc(&f.annexb);
                if au.sps.is_some() {
                    sps = au.sps.clone();
                }
                if au.pps.is_some() {
                    pps = au.pps.clone();
                }
                let key = au.key || f.key;
                if !p.full_video {
                    // Clips only: the replay buffer starts at the first keyframe.
                    if vcfg.is_none() {
                        if !(key && sps.is_some() && pps.is_some()) {
                            continue;
                        }
                        vcfg = Some(VideoConfig { width: p.width, height: p.height, sps: sps.clone().unwrap(), pps: pps.clone().unwrap(), fps: p.fps });
                        for a in pending_audio.drain(..) {
                            replay.push(a);
                        }
                    }
                    if !au.avcc.is_empty() {
                        replay.push(Packet { track: 0, pts: f.pts, data: au.avcc, key });
                    }
                    continue;
                }
                if writer.is_none() && error.is_none() {
                    // The file starts at the first keyframe, when SPS/PPS are known.
                    if !(key && sps.is_some() && pps.is_some()) {
                        continue;
                    }
                    let cfg = VideoConfig { width: p.width, height: p.height, sps: sps.clone().unwrap(), pps: pps.clone().unwrap(), fps: p.fps };
                    match File::create(&p.path).and_then(|f| FragmentedWriter::new(BufWriter::with_capacity(1 << 20, f), cfg.clone(), p.audio.clone())) {
                        Ok(w) => {
                            writer = Some(w);
                            vcfg = Some(cfg);
                            let w = writer.as_mut().unwrap();
                            for a in pending_audio.drain(..) {
                                w.push(a.clone());
                                replay.push(a);
                            }
                        }
                        Err(e) => {
                            error = Some(format!("can't write {}: {e}", p.path.display()));
                            log::error!("{}", error.as_ref().unwrap());
                        }
                    }
                }
                if au.avcc.is_empty() {
                    continue;
                }
                let pkt = Packet { track: 0, pts: f.pts, data: au.avcc, key };
                if let Some(w) = writer.as_mut() {
                    w.push(pkt.clone());
                    replay.push(pkt);
                }
            }
            Some(MuxMsg::Packet(a)) if !p.full_video => {
                if vcfg.is_some() {
                    replay.push(a);
                } else if pending_audio.len() < 2000 {
                    pending_audio.push(a);
                }
            }
            Some(MuxMsg::Packet(a)) => match writer.as_mut() {
                Some(w) => {
                    w.push(a.clone());
                    replay.push(a);
                }
                None => {
                    if pending_audio.len() < 2000 {
                        pending_audio.push(a);
                    }
                }
            },
            Some(MuxMsg::SaveReplay { path, secs, reply }) => {
                let r = match &vcfg {
                    Some(cfg) => {
                        let (pkts, start) = replay.snapshot(secs);
                        File::create(&path)
                            .and_then(|f| write_clip(BufWriter::new(f), cfg, &p.audio, &pkts, start))
                            .map(|_| path.clone())
                            .map_err(|e| anyhow!("saving the clip failed: {e}"))
                    }
                    None => Err(anyhow!("nothing recorded yet")),
                };
                let _ = reply.send(r);
            }
            Some(MuxMsg::Stop { reply }) if !p.full_video => {
                // Nothing on disk to finish; the path doesn't exist (the engine knows).
                let _ = reply.send(Ok((p.path.clone(), 0.0)));
                return;
            }
            Some(MuxMsg::Stop { reply }) => {
                let r = match writer.take() {
                    Some(w) => {
                        let d = w.duration_secs();
                        match w.finish().and_then(|b| b.into_inner().map_err(|e| e.into_error())).and_then(|f| f.sync_all()) {
                            Ok(()) => Ok((p.path.clone(), d)),
                            Err(e) => Err(anyhow!("finishing the recording failed: {e}")),
                        }
                    }
                    None => Err(anyhow!(error.clone().unwrap_or_else(|| "no video was captured (is the game window visible?)".into()))),
                };
                let _ = reply.send(r);
                return;
            }
            None => {}
        }
        // About one fragment per second; force it to disk every 10 s.
        if last_flush.elapsed() >= Duration::from_secs(1) {
            last_flush = Instant::now();
            if let Some(w) = writer.as_mut() {
                if let Err(e) = w.flush_fragment() {
                    log::error!("writing the recording: {e}");
                }
                if last_sync.elapsed() >= Duration::from_secs(10) {
                    last_sync = Instant::now();
                    let _ = w.get_mut().get_ref().sync_data();
                }
            }
        }
    }
}
