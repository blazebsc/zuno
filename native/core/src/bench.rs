//! The benchmark harness. Every framework runs the identical schedule and
//! reports from the identical sampler, so the numbers in
//! `docs/gui-benchmarks/*.md` are comparable.
//!
//! ## Protocol (48 s total)
//!
//! | t (s)    | phase    | what happens                                              |
//! |----------|----------|-----------------------------------------------------------|
//! | 0.0–0.5  | startup  | first frame is up with the 5k library; record at 0.5 s    |
//! | 0.5–8.0  | idle     | no interaction; sample every 1 s                          |
//! | 8.0–28.0 | scroll   | the 5,000-row list follows a triangle wave, top↔bottom,   |
//! |          |          | every frame (worst case: offset jumps, no smooth anim)     |
//! | 28–33    | select   | selection index walks the visible range, every 0.5 s      |
//! | 33–48    | playback | play one synthesized track and let it run; sample 1 Hz    |
//! | 48       | finish   | print the report, exit                                    |
//!
//! RSS is the kernel's `VmRSS` for the app process (all frameworks here are
//! single-process; the runner script adds child processes if any exist).
//! CPU seconds come from `/proc/self/stat`. Peak RSS from `VmHWM`.
//!
//! Run with `--bench`; without it the app is a normal interactive window.

use std::time::Instant;

/// `VmRSS` in KiB, from `/proc/self/status`. Linux-only, which is where the
/// bake-off runs; returns 0 elsewhere so cross-platform builds don't break.
pub fn vm_rss_kb() -> u64 {
    #[cfg(target_os = "linux")]
    {
        if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
            for line in status.lines() {
                if let Some(rest) = line.strip_prefix("VmRSS:") {
                    return rest
                        .trim_end_matches(" kB")
                        .trim()
                        .parse::<u64>()
                        .unwrap_or(0);
                }
            }
        }
        0
    }
    #[cfg(not(target_os = "linux"))]
    {
        0
    }
}

/// High-water mark — the honest "did it ever spike" number.
pub fn vm_hwm_kb() -> u64 {
    #[cfg(target_os = "linux")]
    {
        if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
            for line in status.lines() {
                if let Some(rest) = line.strip_prefix("VmHWM:") {
                    return rest
                        .trim_end_matches(" kB")
                        .trim()
                        .parse::<u64>()
                        .unwrap_or(0);
                }
            }
        }
        0
    }
    #[cfg(not(target_os = "linux"))]
    {
        0
    }
}

/// CPU seconds spent by the process, from `/proc/self/stat` (utime + stime).
pub fn cpu_seconds() -> f64 {
    #[cfg(target_os = "linux")]
    {
        if let Ok(stat) = std::fs::read_to_string("/proc/self/stat") {
            // Fields after the comm field, which may contain spaces but is
            // parenthesized: everything after the final ')'.
            if let Some(i) = stat.rfind(')') {
                let rest: Vec<&str> = stat[i + 1..].split_whitespace().collect();
                // rest[11] is utime, rest[12] stime (stat fields 14, 15).
                if rest.len() > 12 {
                    let ut: f64 = rest[11].parse().unwrap_or(0.0);
                    let st: f64 = rest[12].parse().unwrap_or(0.0);
                    let hz = 100.0; // userspace jiffies; close enough for % CPU
                    return (ut + st) / hz;
                }
            }
        }
        0.0
    }
    #[cfg(not(target_os = "linux"))]
    {
        0.0
    }
}

pub fn thread_count() -> u64 {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_dir("/proc/self/task")
            .map(|d| d.count() as u64)
            .unwrap_or(0)
    }
    #[cfg(not(target_os = "linux"))]
    {
        0
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum BenchAction {
    /// Record the startup RSS now (called once, ~0.5 s after first frame).
    RecordStartup,
    /// Set the big list's scroll position to this fraction (0..1).
    ScrollTo(f32),
    /// Move the selection to this row index.
    SelectRow(usize),
    /// Start playback now (called once).
    StartPlayback,
    /// Bench complete: print the report and exit.
    Finish,
    /// Nothing to do this frame; keep sampling internally.
    Nothing,
}

const T_STARTUP: f32 = 0.5;
const T_IDLE_END: f32 = 8.0;
const T_SCROLL_END: f32 = 28.0;
const T_SELECT_END: f32 = 33.0;
const T_PLAY_END: f32 = 48.0;
/// Full up-and-down sweep of the list during the scroll phase.
const SCROLL_PERIOD: f32 = 4.0;

/// Drives the standard schedule. Construct after the first frame is on screen,
/// call [`BenchDriver::tick`] every frame (or from a ≥10 Hz timer).
pub struct BenchDriver {
    t0: Instant,
    started_cpu: f64,
    startup_recorded: bool,
    playback_started: bool,
    finished: bool,
    last_sample: f32,
    last_select: f32,
    samples: Vec<(f32, &'static str, u64)>,
    startup_kb: u64,
    pub report: String,
}

impl BenchDriver {
    pub fn new() -> Self {
        BenchDriver {
            t0: Instant::now(),
            started_cpu: cpu_seconds(),
            startup_recorded: false,
            playback_started: false,
            finished: false,
            last_sample: -1.0,
            last_select: 0.0,
            samples: Vec::new(),
            startup_kb: 0,
            report: String::new(),
        }
    }

    fn sample(&mut self, phase: &'static str) {
        let kb = vm_rss_kb();
        self.samples.push((self.t0.elapsed().as_secs_f32(), phase, kb));
    }

    /// One step of the schedule. Returns the action the UI must perform.
    pub fn tick(&mut self) -> BenchAction {
        if self.finished {
            return BenchAction::Nothing;
        }
        let t = self.t0.elapsed().as_secs_f32();
        let phase: &'static str = if t < T_IDLE_END {
            "idle"
        } else if t < T_SCROLL_END {
            "scroll"
        } else if t < T_SELECT_END {
            "select"
        } else {
            "playback"
        };

        if t >= T_PLAY_END {
            self.finished = true;
            self.report = self.build_report();
            return BenchAction::Finish;
        }

        let mut action = BenchAction::Nothing;

        if !self.startup_recorded && t >= T_STARTUP {
            self.startup_recorded = true;
            self.startup_kb = vm_rss_kb();
            self.samples.push((t, "startup", self.startup_kb));
            action = BenchAction::RecordStartup;
        }

        // 1 Hz sampling in every phase.
        if t - self.last_sample >= 1.0 {
            self.last_sample = t;
            self.sample(phase);
        }

        if (T_IDLE_END..T_SCROLL_END).contains(&t) {
            // Triangle wave: 0 → 1 → 0 over SCROLL_PERIOD seconds, driven
            // every frame. The UI sets its scroll offset directly — no
            // smooth-scroll easing — which is the worst case for layout.
            let ph = (t - T_IDLE_END) % SCROLL_PERIOD / SCROLL_PERIOD;
            let frac = if ph < 0.5 { ph * 2.0 } else { 2.0 - ph * 2.0 };
            action = BenchAction::ScrollTo(frac);
        } else if (T_SCROLL_END..T_SELECT_END).contains(&t) {
            if t - self.last_select >= 0.5 {
                self.last_select = t;
                // Walk visible-range rows; 0.5 Hz selection changes repaint
                // the selected + previously selected rows.
                let step = ((t - T_SCROLL_END) * 2.0) as usize;
                action = BenchAction::SelectRow((step * 37) % 60);
            }
        }

        if !self.playback_started && t >= T_SELECT_END {
            self.playback_started = true;
            action = BenchAction::StartPlayback;
        }

        action
    }

    /// Markdown report — the exact block each branch pastes into its
    /// `docs/gui-benchmarks/<framework>.md`.
    fn build_report(&self) -> String {
        let total = self.t0.elapsed().as_secs_f32();
        let cpu = cpu_seconds() - self.started_cpu;
        let phase_stats = |name: &str| -> (usize, u64, u64, u64) {
            let s: Vec<u64> = self
                .samples
                .iter()
                .filter(|(t, p, _)| *p == name && *t > 0.4)
                .map(|(_, _, kb)| *kb)
                .collect();
            if s.is_empty() {
                return (0, 0, 0, 0);
            }
            let mean = s.iter().sum::<u64>() / s.len() as u64;
            let min = *s.iter().min().unwrap();
            let max = *s.iter().max().unwrap();
            (s.len(), mean, min, max)
        };
        let mb = |kb: u64| format!("{:.1}", kb as f64 / 1024.0);

        let mut out = String::new();
        out.push_str("## Bench report (auto-generated by `--bench`)\n\n");
        out.push_str("| phase | samples | RSS mean | RSS min | RSS max |\n");
        out.push_str("|-------|---------:|--------:|--------:|--------:|\n");
        let (n, mean, min, max) = phase_stats("startup");
        if n > 0 {
            out.push_str(&format!(
                "| startup (first frame + library) | {n} | {} MB | {} MB | {} MB |\n",
                mb(mean), mb(min), mb(max)
            ));
        }
        for phase in ["idle", "scroll", "select", "playback"] {
            let (n, mean, min, max) = phase_stats(phase);
            if n > 0 {
                out.push_str(&format!(
                    "| {phase} | {n} | {} MB | {} MB | {} MB |\n",
                    mb(mean), mb(min), mb(max)
                ));
            }
        }
        out.push('\n');
        out.push_str(&format!(
            "- Peak RSS (VmHWM): {} MB\n",
            mb(vm_hwm_kb())
        ));
        out.push_str(&format!(
            "- CPU: {:.1}s over {:.0}s wall ({:.0}% of one core)\n",
            cpu,
            total,
            if total > 0.0 { cpu / total as f64 * 100.0 } else { 0.0 }
        ));
        out.push_str(&format!("- Threads at exit: {}\n", thread_count()));
        out
    }
}

impl Default for BenchDriver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rss_reads_nonzero_on_linux() {
        // A running test process has a resident set. HWM is read *after* RSS:
        // other tests allocate in parallel, and RSS can hit a new peak
        // between two reads — HWM sampled first can then be stale-below.
        let rss = vm_rss_kb();
        assert!(rss > 1000);
        assert!(vm_hwm_kb() >= rss);
    }

    #[test]
    fn cpu_seconds_advances() {
        let a = cpu_seconds();
        let busy: u64 = (0..4_000_000_u64).map(|i| i & 0xFFFF).sum();
        assert!(busy > 0);
        let b = cpu_seconds();
        assert!(b >= a);
    }
}
