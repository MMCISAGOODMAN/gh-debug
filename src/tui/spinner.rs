use std::time::{Duration, Instant};

const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub fn spinner_frame(tick: u64) -> &'static str {
    SPINNER[(tick as usize) % SPINNER.len()]
}

pub fn dots(tick: u64) -> String {
    match tick % 3 {
        0 => ".".to_string(),
        1 => "..".to_string(),
        _ => "...".to_string(),
    }
}

pub fn indeterminate_bar(tick: u64, width: usize) -> String {
    let width = width.clamp(8, 36);
    let pos = (tick as usize) % width;
    let mut bar = vec!['─'; width];
    if pos > 0 {
        bar[pos - 1] = '▸';
    }
    bar[pos] = '●';
    if pos + 1 < width {
        bar[pos + 1] = '▹';
    }
    bar.into_iter().collect()
}

pub fn format_elapsed(start: Instant) -> String {
    let d = start.elapsed();
    if d < Duration::from_secs(60) {
        format!("{:.1}s", d.as_secs_f64())
    } else {
        let mins = d.as_secs() / 60;
        let secs = d.as_secs() % 60;
        format!("{}m{:02}s", mins, secs)
    }
}
