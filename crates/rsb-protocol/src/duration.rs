use std::time::Duration;

/// Parse duration strings like `120ms`, `30s`, `5m`, `1h`.
pub fn parse_duration_str(s: &str) -> Option<Duration> {
    let s = s.trim();
    if let Some(ms) = s.strip_suffix("ms") {
        ms.parse::<u64>().ok().map(Duration::from_millis)
    } else if let Some(secs) = s.strip_suffix('s') {
        secs.parse::<u64>().ok().map(Duration::from_secs)
    } else if let Some(mins) = s.strip_suffix('m') {
        mins.parse::<u64>()
            .ok()
            .map(|m| Duration::from_secs(m * 60))
    } else if let Some(hours) = s.strip_suffix('h') {
        hours
            .parse::<u64>()
            .ok()
            .map(|h| Duration::from_secs(h * 3600))
    } else {
        s.parse::<u64>().ok().map(Duration::from_secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ms_before_s() {
        assert_eq!(parse_duration_str("120ms"), Some(Duration::from_millis(120)));
        assert_eq!(parse_duration_str("30s"), Some(Duration::from_secs(30)));
        assert_eq!(parse_duration_str("2m"), Some(Duration::from_secs(120)));
    }
}
