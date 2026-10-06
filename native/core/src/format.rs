//! Duration and count formatting, matching Zuno's exact wording
//! (`MediaHeader`'s "24 songs · 1 hr 32 min", SeekBar's `m:ss`).

/// `3:07` / `1:02:03` — the SeekBar and row durations.
pub fn mmss(sec: u32) -> String {
    let s = sec % 60;
    let m = (sec / 60) % 60;
    let h = sec / 3600;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// "1 hr 32 min" — MediaHeader's total. Drops the minutes at zero when hours
/// are present, keeps "42 min" for short lists.
pub fn long_duration(sec: u32) -> String {
    let h = sec / 3600;
    let m = (sec % 3600) / 60;
    match (h, m) {
        (0, 0) => "0 min".into(),
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} hr"),
        (h, m) => format!("{h} hr {m} min"),
    }
}

/// "24 songs" / "1 song" — MediaHeader counts.
pub fn count_songs(n: usize) -> String {
    if n == 1 {
        "1 song".into()
    } else {
        format!("{n} songs")
    }
}

/// "24 songs · 1 hr 32 min" — the one header meta line.
pub fn counts_line(durations_sec: impl Iterator<Item = u32>) -> String {
    let mut n = 0usize;
    let mut total = 0u32;
    for d in durations_sec {
        n += 1;
        total = total.saturating_add(d);
    }
    format!("{} · {}", count_songs(n), long_duration(total))
}

/// "3.2M listeners" — ArtistView.
pub fn listeners(n: u32) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M listeners", n as f32 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K listeners", n as f32 / 1_000.0)
    } else {
        format!("{n} listeners")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zuno_wording() {
        assert_eq!(mmss(187), "3:07");
        assert_eq!(mmss(3723), "1:02:03");
        assert_eq!(long_duration(5520), "1 hr 32 min");
        assert_eq!(long_duration(2520), "42 min");
        assert_eq!(count_songs(1), "1 song");
        assert_eq!(counts_line([187, 130, 200].into_iter()), "3 songs · 8 min");
    }
}
