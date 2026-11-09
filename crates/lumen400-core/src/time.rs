//! Timecode formatting and parsing.

/// `65.4 → "1:05"`, `3725 → "1:02:05"`. Negative / non-finite values render as `0:00`.
pub fn format_clock(secs: f64) -> String {
    format_clock_with(secs, false)
}

/// Like [`format_clock`] but forces the hour field when `with_hours` is set, which keeps a
/// `current / total` pair visually aligned for long media.
pub fn format_clock_with(secs: f64, with_hours: bool) -> String {
    let total = if secs.is_finite() && secs > 0.0 { secs.floor() as u64 } else { 0 };
    let (h, m, s) = (total / 3600, (total / 60) % 60, total % 60);
    if h > 0 || with_hours { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

/// `3725.25 → "1:02:05.250"`.
pub fn format_clock_ms(secs: f64) -> String {
    let secs = if secs.is_finite() && secs > 0.0 { secs } else { 0.0 };
    let total_ms = (secs * 1000.0).round() as u64;
    let (h, m, s, ms) = (total_ms / 3_600_000, (total_ms / 60_000) % 60, (total_ms / 1000) % 60, total_ms % 1000);
    format!("{h}:{m:02}:{s:02}.{ms:03}")
}

/// Human readable duration: `"1 h 23 min"`, `"45 min"`, `"12 s"`.
pub fn format_duration_long(secs: f64) -> String {
    let total = if secs.is_finite() && secs > 0.0 { secs.round() as u64 } else { 0 };
    let (h, m, s) = (total / 3600, (total / 60) % 60, total % 60);
    match (h, m) {
        (0, 0) => format!("{s} s"),
        (0, _) => {
            if s == 0 {
                format!("{m} min")
            } else {
                format!("{m} min {s} s")
            }
        }
        _ => {
            if m == 0 {
                format!("{h} h")
            } else {
                format!("{h} h {m:02} min")
            }
        }
    }
}

/// `+10 s`, `-2.5 s`, `0 s`.
pub fn format_offset(secs: f64) -> String {
    if secs.abs() < 0.0005 {
        "0 s".to_owned()
    } else if (secs - secs.round()).abs() < 0.0005 {
        format!("{:+} s", secs.round() as i64)
    } else {
        format!("{secs:+.1} s")
    }
}

/// Parses `"90"`, `"90.5"`, `"1:30"`, `"01:02:03.5"` and `"1h2m3s"` style timecodes.
pub fn parse_timecode(input: &str) -> Option<f64> {
    let s = input.trim();
    if s.is_empty() {
        return None;
    }
    if s.contains(':') {
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() > 3 {
            return None;
        }
        let mut total = 0.0;
        for p in &parts {
            let v: f64 = p.trim().parse().ok()?;
            if !v.is_finite() || v < 0.0 {
                return None;
            }
            total = total * 60.0 + v;
        }
        return Some(total);
    }
    if s.chars().any(|c| matches!(c, 'h' | 'm' | 's' | 'H' | 'M' | 'S')) {
        let mut total = 0.0;
        let mut num = String::new();
        for c in s.chars() {
            match c {
                '0'..='9' | '.' => num.push(c),
                'h' | 'H' => total += take(&mut num)? * 3600.0,
                'm' | 'M' => total += take(&mut num)? * 60.0,
                's' | 'S' => total += take(&mut num)?,
                c if c.is_whitespace() => {}
                _ => return None,
            }
        }
        if !num.is_empty() {
            total += take(&mut num)?;
        }
        return Some(total);
    }
    s.parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0)
}

fn take(num: &mut String) -> Option<f64> {
    let v = num.parse::<f64>().ok()?;
    num.clear();
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(format_clock(0.0), "0:00");
        assert_eq!(format_clock(65.9), "1:05");
        assert_eq!(format_clock(3725.0), "1:02:05");
        assert_eq!(format_clock(-4.0), "0:00");
        assert_eq!(format_clock(f64::NAN), "0:00");
        assert_eq!(format_clock_with(65.0, true), "0:01:05");
        assert_eq!(format_clock_ms(3725.25), "1:02:05.250");
        assert_eq!(format_duration_long(5000.0), "1 h 23 min");
        assert_eq!(format_duration_long(45.0), "45 s");
        assert_eq!(format_duration_long(3600.0), "1 h");
        assert_eq!(format_offset(10.0), "+10 s");
        assert_eq!(format_offset(-2.5), "-2.5 s");
        assert_eq!(format_offset(0.0), "0 s");
    }

    #[test]
    fn parses() {
        assert_eq!(parse_timecode("90"), Some(90.0));
        assert_eq!(parse_timecode("1:30"), Some(90.0));
        assert_eq!(parse_timecode("01:02:03.5"), Some(3723.5));
        assert_eq!(parse_timecode("1h2m3s"), Some(3723.0));
        assert_eq!(parse_timecode(" 45s "), Some(45.0));
        assert_eq!(parse_timecode("abc"), None);
        assert_eq!(parse_timecode("-5"), None);
        assert_eq!(parse_timecode("1:2:3:4"), None);
        assert_eq!(parse_timecode(""), None);
    }
}
