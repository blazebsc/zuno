//! The native audio engine for the benchmark.
//!
//! This is the same output stack the shipping app's `rust` engine uses —
//! cpal opens the device, rodio owns the sink (see `src-tauri/src/audio.rs`)
//! — driven by an in-process synthesizer instead of a downloaded Opus stream.
//! Why synthesized: real YouTube stream resolution (Innertube + PO tokens)
//! lives in the TypeScript layer today, and the benchmark must not depend on
//! the network. The engine exercises the parts that matter for the memory
//! measurement: the live 44.1 kHz output stream, a real audio thread, and a
//! real sample pipeline — continuous CPU-side work, no file cache.
//!
//! Architecture follows `audio.rs`: one owned audio thread behind a command
//! channel, because cpal's stream handle is not reliably `Send`, and Tauri
//! state had to be. [`PlayerHandle`] is the `Send + Sync` surface the UI calls.

use crate::model::Track;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

pub const SAMPLE_RATE: u32 = 44_100;

enum Command {
    Play { seed: u64, duration_sec: u32, start_sample: u64 },
    Pause,
    Resume,
    Stop,
    SetVolume(f32),
}

/// Shared state between the audio thread and the handle. `position_samples`
/// is a separate `Arc` so the synthesizer (inside rodio's audio callback) can
/// feed it without touching the rest.
struct Shared {
    position_samples: Arc<AtomicU64>,
    total_samples: AtomicU64,
    playing: AtomicBool,
    /// Set when the current track's samples are exhausted.
    ended: AtomicBool,
}

/// The `Send + Sync` audio controller. Clone-cheap via `Arc`.
pub struct PlayerHandle {
    tx: Sender<Command>,
    shared: Arc<Shared>,
    volume: std::sync::Mutex<f32>,
    pub muted: std::sync::Mutex<bool>,
}

impl PlayerHandle {
    /// Open the default output device and start the audio thread. Infallible:
    /// a missing audio device is handled inside the thread (tracks end
    /// immediately, the app keeps working silently).
    pub fn new() -> Self {
        let (tx, rx) = std::sync::mpsc::channel::<Command>();
        let shared = Arc::new(Shared {
            position_samples: Arc::new(AtomicU64::new(0)),
            total_samples: AtomicU64::new(0),
            playing: AtomicBool::new(false),
            ended: AtomicBool::new(false),
        });
        let thread_shared = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("zuno-audio".into())
            .stack_size(1 << 20)
            .spawn(move || audio_thread(rx, thread_shared, 0.5))
            .expect("audio thread spawns");
        PlayerHandle {
            tx,
            shared,
            volume: std::sync::Mutex::new(0.7),
            muted: std::sync::Mutex::new(false),
        }
    }

    /// Load `track` and start playing from the beginning (or `start_sec`).
    pub fn play(&self, track: &Track, seed: u64, start_sec: f64) {
        let start = (start_sec.max(0.0) * SAMPLE_RATE as f64) as u64;
        self.shared
            .ended
            .store(false, Ordering::Release);
        let _ = self.tx.send(Command::Play {
            seed,
            duration_sec: track.duration_sec,
            start_sample: start,
        });
        self.shared.playing.store(true, Ordering::Release);
    }

    pub fn pause(&self) {
        let _ = self.tx.send(Command::Pause);
        self.shared.playing.store(false, Ordering::Release);
    }

    pub fn resume(&self) {
        let _ = self.tx.send(Command::Resume);
        self.shared.playing.store(true, Ordering::Release);
    }

    pub fn stop(&self) {
        let _ = self.tx.send(Command::Stop);
        self.shared.playing.store(false, Ordering::Release);
        self.shared.position_samples.store(0, Ordering::Release);
        self.shared.total_samples.store(0, Ordering::Release);
    }

    /// Seek within the loaded track. Recreates the source at the offset —
    /// same trick the shipping engine uses for a seek with no decoder
    /// rewind support.
    pub fn seek(&self, track: &Track, seed: u64, sec: f64) {
        let pos = (sec.max(0.0) * SAMPLE_RATE as f64) as u64;
        self.shared.ended.store(false, Ordering::Release);
        let _ = self.tx.send(Command::Play {
            seed,
            duration_sec: track.duration_sec,
            start_sample: pos,
        });
        self.shared.playing.store(true, Ordering::Release);
    }

    pub fn set_volume(&self, v: f32) {
        *self.volume.lock().unwrap() = v.clamp(0.0, 1.0);
        let m = *self.muted.lock().unwrap();
        let _ = self.tx.send(Command::SetVolume(if m { 0.0 } else { v.clamp(0.0, 1.0) }));
    }

    pub fn volume(&self) -> f32 {
        *self.volume.lock().unwrap()
    }

    pub fn set_muted(&self, m: bool) {
        *self.muted.lock().unwrap() = m;
        let v = *self.volume.lock().unwrap();
        let _ = self.tx.send(Command::SetVolume(if m { 0.0 } else { v }));
    }

    pub fn is_muted(&self) -> bool {
        *self.muted.lock().unwrap()
    }

    pub fn is_playing(&self) -> bool {
        self.shared.playing.load(Ordering::Acquire)
    }

    /// Playhead in seconds. Sample-counter-derived, not wall-clock — pauses
    /// hold position, seeks jump it, exactly like the cached playhead in
    /// `src/player/rustAudio.ts`.
    pub fn position_sec(&self) -> f64 {
        self.shared.position_samples.load(Ordering::Acquire) as f64 / SAMPLE_RATE as f64
    }

    pub fn duration_sec(&self) -> f64 {
        self.shared.total_samples.load(Ordering::Acquire) as f64 / SAMPLE_RATE as f64
    }

    /// Drain the "track finished" signal. Returns the flag once.
    pub fn take_ended(&self) -> bool {
        self.shared.ended.swap(false, Ordering::AcqRel)
    }
}

fn audio_thread(rx: Receiver<Command>, shared: Arc<Shared>, initial_volume: f32) {
    // Opening the sink can fail (headless CI, no ALSA). The app keeps
    // working without sound; tracks just end immediately so the queue
    // keeps advancing.
    let stream = rodio::stream::DeviceSinkBuilder::open_default_sink();
    let (stream, sink) = match stream {
        Ok(s) => {
            let player = rodio::Player::connect_new(s.mixer());
            (s, player)
        }
        Err(e) => {
            eprintln!("[zuno-core] audio device unavailable: {e}");
            let shared2 = Arc::clone(&shared);
            std::thread::spawn(move || {
                // Drain commands so sends never block; emulate track end.
                while let Ok(cmd) = rx.recv() {
                    if matches!(cmd, Command::Play { .. }) {
                        shared2.playing.store(false, Ordering::Release);
                        shared2.ended.store(true, Ordering::Release);
                    }
                }
            });
            return;
        }
    };
    // Keep the stream alive for as long as the thread runs — the Player
    // plays into its mixer (same shape as `audio.rs`'s `_stream` field).
    let _stream = stream;
    sink.set_volume(initial_volume);
    let mut loaded_total = 0u64;
    let mut active = false;

    loop {
        // Poll commands with the same 250 ms cadence `audio.rs` uses for its
        // position tick.
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(Command::Play { seed, duration_sec, start_sample }) => {
                let total = duration_sec as u64 * SAMPLE_RATE as u64;
                sink.clear();
                let src = SynthSource::new(seed, start_sample, total, Arc::clone(&shared.position_samples));
                sink.append(src);
                sink.play();
                loaded_total = total;
                shared.total_samples.store(total, Ordering::Release);
                shared.position_samples.store(start_sample, Ordering::Release);
                shared.ended.store(false, Ordering::Release);
                active = total > start_sample;
            }
            Ok(Command::Pause) => sink.pause(),
            Ok(Command::Resume) => sink.play(),
            Ok(Command::Stop) => {
                sink.clear();
                sink.pause();
                active = false;
                shared.position_samples.store(0, Ordering::Release);
                shared.total_samples.store(0, Ordering::Release);
            }
            Ok(Command::SetVolume(v)) => sink.set_volume(v),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if active {
            let pos = shared.position_samples.load(Ordering::Acquire);
            if loaded_total > 0 && pos >= loaded_total {
                active = false;
                sink.pause();
                shared.playing.store(false, Ordering::Release);
                shared.ended.store(true, Ordering::Release);
            }
        }
    }
}

// — The synthesizer ————————————————————————————————————————————

/// A synthesized track: seeded chord progression + bass + arpeggio + hats,
/// mellow and quiet on purpose (a benchmark runs unsupervised on someone's
/// desk). Implements `rodio::Source` over `f32` mono at [`SAMPLE_RATE`].
///
/// The source also feeds the shared position counter (every 1024 samples —
/// an atomic store per sample would put a contended write in the audio
/// callback for no benefit; 1024 samples is 23 ms of resolution).
pub struct SynthSource {
    seed: u64,
    total: u64,
    pos: u64,
    pos_out: Arc<AtomicU64>,
    state: SynthState,
}

struct SynthState {
    tempo_bpm: f32,
    /// Chord progression as semitone offsets from the root.
    chords: Vec<[i32; 3]>,
    root_hz: f32,
    scale: Vec<i32>,
}

impl SynthState {
    fn new(seed: u64) -> Self {
        let progressions: [&[[i32; 3]]; 6] = [
            &[[0, 4, 7], [9, 12, 16], [5, 9, 12], [7, 11, 14]],
            &[[0, 3, 7], [8, 12, 15], [5, 8, 12], [3, 7, 10]],
            &[[0, 4, 7], [5, 9, 12], [7, 11, 14], [2, 5, 9]],
            &[[0, 3, 7], [3, 7, 10], [8, 12, 15], [10, 13, 17]],
            &[[0, 5, 9], [7, 11, 14], [2, 6, 9], [4, 7, 11]],
            &[[0, 4, 7], [2, 5, 9], [4, 7, 11], [7, 11, 14]],
        ];
        let chords: Vec<[i32; 3]> = progressions[(seed >> 20) as usize % progressions.len()]
            .iter()
            .copied()
            .collect();
        let roots = [110.0, 116.54, 123.47, 130.81, 146.83, 155.56, 164.81, 174.61];
        SynthState {
            tempo_bpm: 84.0 + (crate::rng::hash(seed >> 8) % 56) as f32,
            chords,
            root_hz: roots[(seed >> 4) as usize % roots.len()],
            scale: vec![0, 2, 3, 5, 7, 8, 10, 12],
        }
    }
}

const TAU: f32 = std::f32::consts::TAU;

impl SynthSource {
    pub fn new(seed: u64, start_sample: u64, total_samples: u64, pos_out: Arc<AtomicU64>) -> Self {
        SynthSource {
            seed,
            total: total_samples,
            pos: start_sample,
            pos_out,
            state: SynthState::new(seed),
        }
    }

    fn sample_at(&self, n: u64) -> f32 {
        let t = n as f32 / SAMPLE_RATE as f32;
        let beat = 60.0 / self.state.tempo_bpm;
        let bar = beat * 4.0;
        let chord_idx = ((t / bar).floor() as usize) % self.state.chords.len();
        let chord = self.state.chords[chord_idx];
        let root = self.state.root_hz;

        // Pad: three detuned sines on the chord, soft-attacked each bar.
        let mut s = 0.0f32;
        for &semi in &chord {
            let f = root * 2.0f32.powf(semi as f32 / 12.0);
            s += (TAU * f * t).sin() * 0.10;
            s += (TAU * f * 1.003 * t + 0.4).sin() * 0.05;
        }
        // Slow amplitude swell per bar.
        let in_bar = (t % bar) / bar;
        let swell = 0.6 + 0.4 * (std::f32::consts::PI * in_bar).sin();
        s *= swell;

        // Bass: root every beat with a pluck envelope.
        let beat_pos = (t / beat).fract();
        let bass_env = (-beat_pos * 6.0).exp();
        let bass_f = root / 2.0;
        s += (TAU * bass_f * t).sin() * 0.16 * bass_env;

        // Arpeggio: a scale note per 8th, plucked.
        let eighth = beat / 2.0;
        let step = (t / eighth) as u64;
        let note_pos = (t % eighth) / eighth;
        let pluck = (-note_pos * 9.0).exp();
        let semi = self.state.scale[(crate::rng::hash(self.seed ^ step) as usize) % self.state.scale.len()];
        let arp_f = root * 2.0 * 2.0f32.powf(semi as f32 / 12.0);
        s += (TAU * arp_f * t).sin() * 0.07 * pluck;

        // Hat: filtered noise ticks on offbeats.
        let offbeat = ((t / (beat / 2.0)) as u64) % 2 == 1;
        if offbeat {
            let noise = crate::rng::hash(self.seed ^ (n >> 11)) as f32 / u64::MAX as f32 - 0.5;
            s += noise * 0.012 * pluck;
        }

        // Keep it gentle: soft-clip and scale.
        s.tanh() * 0.55
    }
}

impl Iterator for SynthSource {
    type Item = f32;
    #[inline]
    fn next(&mut self) -> Option<f32> {
        if self.pos >= self.total {
            // Park the playhead exactly at the end so the audio thread's
            // `pos >= total` check fires on its next 250 ms poll.
            self.pos_out.store(self.total, Ordering::Relaxed);
            return None;
        }
        let v = self.sample_at(self.pos);
        self.pos += 1;
        if self.pos & 1023 == 0 {
            self.pos_out.store(self.pos, Ordering::Relaxed);
        }
        Some(v)
    }
}

impl rodio::Source for SynthSource {
    #[inline]
    fn current_span_len(&self) -> Option<usize> {
        Some((self.total - self.pos) as usize)
    }
    #[inline]
    fn channels(&self) -> rodio::ChannelCount {
        std::num::NonZero::new(1).unwrap()
    }
    #[inline]
    fn sample_rate(&self) -> rodio::SampleRate {
        std::num::NonZero::new(SAMPLE_RATE).unwrap()
    }
    #[inline]
    fn total_duration(&self) -> Option<Duration> {
        Some(Duration::from_secs_f64(self.total as f64 / SAMPLE_RATE as f64))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synth_is_bounded_and_deterministic() {
        let pos = Arc::new(AtomicU64::new(0));
        let a = SynthSource::new(1234, 0, 4410, Arc::clone(&pos));
        let b = SynthSource::new(1234, 0, 4410, Arc::clone(&pos));
        let va: Vec<f32> = a.take(2000).collect();
        let vb: Vec<f32> = b.take(2000).collect();
        assert_eq!(va, vb);
        assert!(va.iter().all(|&s| s.abs() <= 1.0));
        // Position counter advanced in 1024-sample steps.
        assert_eq!(pos.load(Ordering::Relaxed), 1024);
    }

    #[test]
    fn synth_respects_start_offset_and_parks_position() {
        let pos = Arc::new(AtomicU64::new(0));
        let mut src = SynthSource::new(99, 44_100, 44_100 * 10, Arc::clone(&pos));
        assert_eq!(rodio::Source::current_span_len(&src), Some(44_100 * 9));
        // Drain to the end: position parks exactly at total.
        while src.next().is_some() {}
        assert_eq!(pos.load(Ordering::Relaxed), 44_100 * 10);
    }
}
