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
    PlayStream { url: String, mime: String, cookie: Option<String> },
    SeekStream { sec: f64 },
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
    /// True while a network stream (not the synthesizer) is loaded.
    streaming: AtomicBool,
    /// Wall-clock playhead for streams; the synth path uses sample counters.
    stream_clock: std::sync::Mutex<StreamClock>,
}

#[derive(Default)]
struct StreamClock {
    /// Milliseconds banked before the current run started.
    acc_ms: u64,
    /// When the current run started (`None` while paused).
    since: Option<std::time::Instant>,
    /// Track length, 0 when the container did not say.
    duration_ms: u64,
}

impl StreamClock {
    fn elapsed_ms(&self) -> u64 {
        self.acc_ms
            + self.since.map(|t| t.elapsed().as_millis() as u64).unwrap_or(0)
    }
    fn pause(&mut self) {
        self.acc_ms = self.elapsed_ms();
        self.since = None;
    }
    fn resume(&mut self) {
        if self.since.is_none() {
            self.since = Some(std::time::Instant::now());
        }
    }
    fn restart(&mut self, duration_ms: u64) {
        self.acc_ms = 0;
        self.since = Some(std::time::Instant::now());
        self.duration_ms = duration_ms;
    }
    fn seek(&mut self, ms: u64) {
        self.acc_ms = ms;
        self.since = Some(std::time::Instant::now());
    }
    fn reset(&mut self) {
        *self = StreamClock::default();
    }
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
            streaming: AtomicBool::new(false),
            stream_clock: std::sync::Mutex::new(StreamClock::default()),
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

    /// Play a resolved stream URL (see `zuno-yt::resolve_stream`). The audio
    /// thread downloads the bytes (`Range: bytes=0-`), decodes through
    /// symphonia/libopus, and appends to the same sink the synth path uses.
    /// A failed download ends the track (playing=false, ended=true) so the
    /// queue keeps advancing instead of hanging.
    pub fn play_stream(&self, url: &str, mime: &str, cookie: Option<&str>) {
        self.shared.ended.store(false, Ordering::Release);
        self.shared.streaming.store(true, Ordering::Release);
        let _ = self.tx.send(Command::PlayStream {
            url: url.to_string(),
            mime: mime.to_string(),
            cookie: cookie.map(str::to_string),
        });
        self.shared.playing.store(true, Ordering::Release);
    }

    /// Seek within the loaded stream. Recreates the decoder at the offset.
    pub fn seek_stream(&self, sec: f64) {
        self.shared.ended.store(false, Ordering::Release);
        let _ = self.tx.send(Command::SeekStream { sec: sec.max(0.0) });
        self.shared.playing.store(true, Ordering::Release);
    }

    pub fn is_streaming(&self) -> bool {
        self.shared.streaming.load(Ordering::Acquire)
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

    /// Playhead in seconds. Sample-counter-derived for synth, wall-clock
    /// for streams — pauses hold position, seeks jump it, exactly like the
    /// cached playhead in `src/player/rustAudio.ts`.
    pub fn position_sec(&self) -> f64 {
        if self.shared.streaming.load(Ordering::Acquire) {
            self.shared.stream_clock.lock().unwrap().elapsed_ms() as f64 / 1000.0
        } else {
            self.shared.position_samples.load(Ordering::Acquire) as f64 / SAMPLE_RATE as f64
        }
    }

    pub fn duration_sec(&self) -> f64 {
        if self.shared.streaming.load(Ordering::Acquire) {
            self.shared.stream_clock.lock().unwrap().duration_ms as f64 / 1000.0
        } else {
            self.shared.total_samples.load(Ordering::Acquire) as f64 / SAMPLE_RATE as f64
        }
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
    // The loaded stream's bytes, kept for seeks (decoder recreated at offset).
    let mut stream_bytes: Option<(Arc<Vec<u8>>, String)> = None;

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
                shared.streaming.store(false, Ordering::Release);
                stream_bytes = None;
                active = total > start_sample;
            }
            Ok(Command::PlayStream { url, mime, cookie }) => {
                match download_stream(&url, cookie.as_deref()) {
                    Ok(bytes) => {
                        let bytes = Arc::new(bytes);
                        match open_stream_source(Arc::clone(&bytes), &mime) {
                            Some(src) => {
                                let ms = src.duration().map(|d| d.as_millis() as u64).unwrap_or(0);
                                sink.clear();
                                src.append_to(&sink);
                                sink.play();
                                shared.stream_clock.lock().unwrap().restart(ms);
                                shared.total_samples.store(0, Ordering::Release);
                                shared.ended.store(false, Ordering::Release);
                                shared.streaming.store(true, Ordering::Release);
                                stream_bytes = Some((bytes, mime));
                                loaded_total = 0;
                                active = true;
                            }
                            None => {
                                eprintln!("[zuno-core] stream decode failed");
                                shared.playing.store(false, Ordering::Release);
                                shared.ended.store(true, Ordering::Release);
                                active = false;
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("[zuno-core] stream download failed: {e}");
                        shared.playing.store(false, Ordering::Release);
                        shared.ended.store(true, Ordering::Release);
                        active = false;
                    }
                }
            }
            Ok(Command::SeekStream { sec }) => {
                if let Some((bytes, mime)) = stream_bytes.clone() {
                    if let Some(mut src) = open_stream_source(bytes, &mime) {
                        let _ = src.try_seek(Duration::from_secs_f64(sec));
                        sink.clear();
                        src.append_to(&sink);
                        sink.play();
                        shared.stream_clock.lock().unwrap().seek((sec * 1000.0) as u64);
                        shared.ended.store(false, Ordering::Release);
                        active = true;
                    }
                }
            }
            Ok(Command::Pause) => {
                sink.pause();
                shared.stream_clock.lock().unwrap().pause();
            }
            Ok(Command::Resume) => {
                sink.play();
                shared.stream_clock.lock().unwrap().resume();
            }
            Ok(Command::Stop) => {
                sink.clear();
                sink.pause();
                active = false;
                shared.streaming.store(false, Ordering::Release);
                shared.stream_clock.lock().unwrap().reset();
                stream_bytes = None;
                shared.position_samples.store(0, Ordering::Release);
                shared.total_samples.store(0, Ordering::Release);
            }
            Ok(Command::SetVolume(v)) => sink.set_volume(v),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if active {
            if shared.streaming.load(Ordering::Acquire) {
                // Streams end by duration or by decoder exhaustion (truncated
                // downloads finish their buffered frames, then the sink drains).
                let (pos_ms, dur_ms) = {
                    let clock = shared.stream_clock.lock().unwrap();
                    (clock.elapsed_ms(), clock.duration_ms)
                };
                if (dur_ms > 0 && pos_ms >= dur_ms) || sink.empty() {
                    active = false;
                    sink.pause();
                    shared.playing.store(false, Ordering::Release);
                    shared.ended.store(true, Ordering::Release);
                }
            } else {
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
}

// — Network stream decode —————————————————————————————————————
// Port of the decode path in `src-tauri/src/audio.rs` (BufferReader as a
// MediaSource) + `src-tauri/src/opus_source.rs` (symphonia demuxes, libopus
// decodes Opus; everything else goes to rodio's Decoder). Simplified: the
// whole body is downloaded first (tracks are a few MB), so the progressive
// MediaBuffer becomes an in-memory VecSource.
//
// Opus decodes to f32, rodio's Decoder to i16 — different `Source::Item`
// types, so the two ride in an enum and each appends directly to the sink
// (`append` is generic; no unified box type needed).

/// `Range: bytes=0-` download into memory. Ranged like the browser asks;
/// googlevideo refuses headerless full-file fetches on signed URLs.
fn download_stream(url: &str, cookie: Option<&str>) -> Result<Vec<u8>, String> {
    let mut req = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| e.to_string())?
        .get(url)
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/135.0.0.0 Safari/537.36")
        .header("Accept", "*/*")
        .header("Accept-Language", "en-US,en;q=0.9")
        .header("Accept-Encoding", "identity;q=1, *;q=0")
        .header("Range", "bytes=0-");
    if let Some(cookie) = cookie {
        req = req.header("Cookie", cookie);
    }
    let resp = req.send().map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("stream HTTP {}", resp.status()));
    }
    resp.bytes().map(|b| b.to_vec()).map_err(|e| e.to_string())
}

/// In-memory `Read + Seek + MediaSource` for symphonia probing.
struct VecSource {
    bytes: Arc<Vec<u8>>,
    pos: u64,
}

impl VecSource {
    fn new(bytes: Arc<Vec<u8>>) -> Self {
        VecSource { bytes, pos: 0 }
    }
}

impl std::io::Read for VecSource {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        let start = self.pos as usize;
        if start >= self.bytes.len() {
            return Ok(0);
        }
        let n = (self.bytes.len() - start).min(out.len());
        out[..n].copy_from_slice(&self.bytes[start..start + n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl std::io::Seek for VecSource {
    fn seek(&mut self, from: std::io::SeekFrom) -> std::io::Result<u64> {
        let target = match from {
            std::io::SeekFrom::Start(o) => o as i64,
            std::io::SeekFrom::Current(d) => self.pos as i64 + d,
            std::io::SeekFrom::End(d) => self.bytes.len() as i64 + d,
        };
        if target < 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "seek before start",
            ));
        }
        self.pos = (target as usize).min(self.bytes.len()) as u64;
        Ok(self.pos)
    }
}

impl symphonia::core::io::MediaSource for VecSource {
    fn is_seekable(&self) -> bool {
        true
    }
    fn byte_len(&self) -> Option<u64> {
        Some(self.bytes.len() as u64)
    }
}

/// Whether only libopus can decode this (port of `is_opus`).
fn is_opus(mime_type: &str) -> bool {
    let lowered = mime_type.to_ascii_lowercase();
    lowered.contains("opus") || lowered.contains("webm")
}

/// Container sniffed from magic bytes (port of `container_mime_of`): the
/// declared mime describes what was resolved for streaming, so stored bytes
/// are routed by what they are. Rewinds before returning.
fn sniff_container_mime(
    reader: &mut (impl std::io::Read + std::io::Seek),
) -> std::io::Result<Option<&'static str>> {
    let mut header = [0u8; 64];
    let mut filled = 0;
    while filled < header.len() {
        match reader.read(&mut header[filled..]) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    reader.seek(std::io::SeekFrom::Start(0))?;
    Ok(container_mime_of(&header[..filled]))
}

fn container_mime_of(header: &[u8]) -> Option<&'static str> {
    if header.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return Some("audio/webm; codecs=\"opus\"");
    }
    if header.starts_with(b"OggS") {
        return Some(if header.windows(8).any(|window| window == b"OpusHead") {
            "audio/opus"
        } else {
            "audio/ogg"
        });
    }
    if header.len() >= 12 && &header[4..8] == b"ftyp" {
        return Some("audio/mp4");
    }
    if header.starts_with(b"fLaC") {
        return Some("audio/flac");
    }
    None
}

/// Decode `bytes` to a sink-ready source.
fn open_stream_source(bytes: Arc<Vec<u8>>, mime: &str) -> Option<StreamSource> {
    let mut probe = VecSource::new(Arc::clone(&bytes));
    let effective = sniff_container_mime(&mut probe).ok().flatten().unwrap_or(mime);
    if is_opus(effective) {
        match OpusSource::new(Box::new(VecSource::new(Arc::clone(&bytes))), effective) {
            Ok(src) => Some(StreamSource::Opus(src)),
            Err(e) => {
                eprintln!("[zuno-core] opus open failed ({e}), trying rodio");
                open_rodio(bytes)
            }
        }
    } else {
        open_rodio(bytes)
    }
}

enum StreamSource {
    Opus(OpusSource),
    Other(rodio::Decoder<std::io::Cursor<Vec<u8>>>),
}

impl StreamSource {
    fn duration(&self) -> Option<Duration> {
        use rodio::Source as _;
        match self {
            StreamSource::Opus(src) => src.total_duration(),
            StreamSource::Other(dec) => dec.total_duration(),
        }
    }

    fn try_seek(&mut self, pos: Duration) -> bool {
        use rodio::Source as _;
        match self {
            StreamSource::Opus(src) => src.try_seek(pos).is_ok(),
            StreamSource::Other(dec) => dec.try_seek(pos).is_ok(),
        }
    }

    fn append_to(self, sink: &rodio::Player) {
        match self {
            StreamSource::Opus(src) => sink.append(src),
            StreamSource::Other(dec) => sink.append(dec),
        }
    }
}

fn open_rodio(bytes: Arc<Vec<u8>>) -> Option<StreamSource> {
    let cursor = std::io::Cursor::new(bytes.to_vec());
    match rodio::Decoder::new(cursor) {
        Ok(decoder) => Some(StreamSource::Other(decoder)),
        Err(e) => {
            eprintln!("[zuno-core] rodio decode failed: {e}");
            None
        }
    }
}

// — Opus decode (symphonia demuxes, libopus decodes) —————————————————
// Port of `src-tauri/src/opus_source.rs`; the progressive stall handling
// collapses because the body is already in memory.

/// Opus always decodes at 48 kHz (RFC 6716).
const OPUS_SAMPLE_RATE: u32 = 48_000;

/// Longest Opus frame: 120 ms at 48 kHz = 5760 samples per channel.
const MAX_FRAME_SAMPLES: usize = 5_760;

struct OpusSource {
    format: Box<dyn symphonia::core::formats::FormatReader>,
    decoder: opus::Decoder,
    track_id: u32,
    channels: rodio::ChannelCount,
    pending: std::collections::VecDeque<f32>,
    scratch: Vec<f32>,
    total_duration: Option<Duration>,
    skip_samples: usize,
    exhausted: bool,
}

impl OpusSource {
    fn new(
        source: Box<dyn symphonia::core::io::MediaSource>,
        mime_type: &str,
    ) -> std::result::Result<Self, String> {
        use symphonia::core::formats::FormatOptions;
        use symphonia::core::io::MediaSourceStream;
        use symphonia::core::meta::MetadataOptions;
        use symphonia::core::probe::Hint;
        let stream = MediaSourceStream::new(source, Default::default());
        let mut hint = Hint::new();
        if mime_type.contains("webm") || mime_type.contains("matroska") {
            hint.with_extension("webm");
        } else if mime_type.contains("ogg") || mime_type.contains("opus") {
            hint.with_extension("ogg");
        }
        let probed = symphonia::default::get_probe()
            .format(&hint, stream, &FormatOptions { enable_gapless: true, ..Default::default() }, &MetadataOptions::default())
            .map_err(|e| format!("opus container probe failed: {e}"))?;
        let format = probed.format;
        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec == symphonia::core::codecs::CODEC_TYPE_OPUS)
            .ok_or_else(|| "container holds no Opus track".to_string())?;
        let params = &track.codec_params;
        let track_id = track.id;
        let channel_count = params.channels.map(|c| c.count()).unwrap_or(2).max(1);
        let channels = rodio::ChannelCount::new(channel_count as u16)
            .ok_or_else(|| "opus track declared zero channels".to_string())?;
        let opus_channels = match channel_count {
            1 => opus::Channels::Mono,
            2 => opus::Channels::Stereo,
            other => return Err(format!("unsupported opus channel count: {other}")),
        };
        let decoder =
            opus::Decoder::new(OPUS_SAMPLE_RATE, opus_channels).map_err(|e| format!("opus decoder init failed: {e}"))?;
        let total_duration = params
            .time_base
            .zip(params.n_frames)
            .and_then(|(tb, frames)| {
                if tb.numer == 0 || tb.denom == 0 {
                    return None;
                }
                let time = tb.calc_time(frames);
                Some(Duration::from_secs_f64(time.seconds as f64 + time.frac))
            });
        let skip_samples = params.delay.unwrap_or(0) as usize * channel_count;
        Ok(OpusSource {
            format,
            decoder,
            track_id,
            channels,
            pending: std::collections::VecDeque::with_capacity(MAX_FRAME_SAMPLES * channel_count),
            scratch: vec![0.0; MAX_FRAME_SAMPLES * channel_count],
            total_duration,
            skip_samples,
            exhausted: false,
        })
    }

    fn fill(&mut self) -> bool {
        use symphonia::core::errors::Error as SymphoniaError;
        while !self.exhausted {
            // In-memory body: any read error is end-of-stream (or a corrupt
            // packet stream with nothing more to decode).
            let packet = match self.format.next_packet() {
                Ok(packet) => packet,
                Err(e) => {
                    if !matches!(e, SymphoniaError::IoError(_)) {
                        eprintln!("[zuno-core] opus demux ended: {e}");
                    }
                    self.exhausted = true;
                    return false;
                }
            };
            if packet.track_id() != self.track_id {
                continue;
            }
            let frames = match self.decoder.decode_float(&packet.data, &mut self.scratch, false) {
                Ok(frames) => frames,
                Err(e) => {
                    eprintln!("[zuno-core] opus packet dropped: {e}");
                    continue;
                }
            };
            let sample_count = frames * self.channels.get() as usize;
            let mut decoded = &self.scratch[..sample_count.min(self.scratch.len())];
            if self.skip_samples > 0 {
                let skipped = self.skip_samples.min(decoded.len());
                self.skip_samples -= skipped;
                decoded = &decoded[skipped..];
            }
            if decoded.is_empty() {
                continue;
            }
            self.pending.extend(decoded.iter().copied());
            return true;
        }
        false
    }
}

impl Iterator for OpusSource {
    type Item = f32;
    #[inline]
    fn next(&mut self) -> Option<f32> {
        if let Some(sample) = self.pending.pop_front() {
            return Some(sample);
        }
        if !self.fill() {
            return None;
        }
        self.pending.pop_front()
    }
}

impl rodio::Source for OpusSource {
    #[inline]
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    #[inline]
    fn channels(&self) -> rodio::ChannelCount {
        self.channels
    }
    #[inline]
    fn sample_rate(&self) -> rodio::SampleRate {
        rodio::SampleRate::new(OPUS_SAMPLE_RATE).expect("48 kHz is not zero")
    }
    #[inline]
    fn total_duration(&self) -> Option<Duration> {
        self.total_duration
    }
    fn try_seek(&mut self, position: Duration) -> std::result::Result<(), rodio::source::SeekError> {
        use symphonia::core::formats::{SeekMode, SeekTo};
        use symphonia::core::units::Time;
        self.format
            .seek(
                SeekMode::Coarse,
                SeekTo::Time { time: Time::from(position.as_secs_f64()), track_id: Some(self.track_id) },
            )
            .map_err(|e| {
                eprintln!("[zuno-core] opus seek failed: {e}");
                rodio::source::SeekError::NotSupported { underlying_source: "OpusSource" }
            })?;
        self.pending.clear();
        self.exhausted = false;
        let _ = self.decoder.reset_state();
        Ok(())
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
