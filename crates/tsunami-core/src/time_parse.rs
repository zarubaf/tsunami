//! Time string parser — converts human-readable time strings to picoseconds.

use std::sync::OnceLock;
use regex::Regex;

static TIME_RE: OnceLock<Regex> = OnceLock::new();

fn time_re() -> &'static Regex {
    TIME_RE.get_or_init(|| {
        Regex::new(r"(?i)^\s*([0-9]*\.?[0-9]+)\s*(ps|ns|us|µs|ms|s|cyc)?\s*$").unwrap()
    })
}

fn multiplier(unit: &str) -> Option<f64> {
    match unit {
        "ps" => Some(1.0),
        "ns" => Some(1_000.0),
        "us" | "µs" => Some(1_000_000.0),
        "ms" => Some(1_000_000_000.0),
        "s" => Some(1_000_000_000_000.0),
        _ => None,
    }
}

/// Parse a time value into picoseconds.
///
/// Accepts integers (raw passthrough), or strings like `"1284ns"`, `"1.284us"`, `"642cyc"`.
/// The `timescale_ps` argument is required when using the `"cyc"` unit.
pub fn parse_time(value: &str, timescale_ps: Option<u64>) -> Result<u64, String> {
    let value = value.trim();

    // Try plain integer first
    if let Ok(n) = value.parse::<u64>() {
        return Ok(n);
    }

    let re = time_re();
    let caps = re.captures(value).ok_or_else(|| format!("Cannot parse time: {value:?}"))?;

    let number: f64 = caps[1].parse().map_err(|_| format!("Cannot parse time: {value:?}"))?;
    let unit = match caps.get(2) {
        Some(m) => m.as_str(),
        None => return Ok(number as u64),
    };

    let unit_lower = unit.to_lowercase();
    let unit_key = if unit == "µs" { "µs" } else { &unit_lower };

    if unit_key == "cyc" {
        let ts = timescale_ps.ok_or("Cannot convert cycles without timescale_ps")?;
        return Ok((number * ts as f64) as u64);
    }

    let mult = multiplier(unit_key).ok_or_else(|| format!("Unknown time unit: {unit:?}"))?;
    Ok((number * mult) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_integer() {
        assert_eq!(parse_time("1234", None).unwrap(), 1234);
    }

    #[test]
    fn test_parse_ns() {
        assert_eq!(parse_time("1284ns", None).unwrap(), 1_284_000);
    }

    #[test]
    fn test_parse_us() {
        assert_eq!(parse_time("1.284us", None).unwrap(), 1_284_000);
    }

    #[test]
    fn test_parse_cyc() {
        assert_eq!(parse_time("642cyc", Some(1000)).unwrap(), 642_000);
    }

    #[test]
    fn test_parse_cyc_without_timescale() {
        assert!(parse_time("642cyc", None).is_err());
    }
}
