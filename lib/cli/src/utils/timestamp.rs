use time::{
    format_description::well_known::{Rfc2822, Rfc3339},
    Date, OffsetDateTime, PrimitiveDateTime, Time,
};

/// Try to parse the string as a timestamp in a number of well-known formats.
///
/// Supported formats,
///
/// - RFC 3339 (`2006-01-02T03:04:05-07:00`)
/// - RFC 2822 (`Mon, 02 Jan 2006 03:04:05 MST`)
/// - Date (`2006-01-02`)
/// - Unix timestamp (`1136196245`)
/// - Relative time (`10m` / `-1h`, `1d`)
pub fn parse_timestamp_or_relative_time(
    s: &str,
    assume_negative_offset: bool,
) -> Result<OffsetDateTime, anyhow::Error> {
    parse_timestamp_or_relative_time_based(s, OffsetDateTime::now_utc(), assume_negative_offset)
}

/// See [`parse_timestamp_or_relative_time`].
///
/// NOTE: assumes a negative offset if time is specified in a format like "1h".
pub fn parse_timestamp_or_relative_time_negative_offset(
    s: &str,
) -> Result<OffsetDateTime, anyhow::Error> {
    parse_timestamp_or_relative_time(s, true)
}

/// See [`parse_timestamp_or_relative_time`].
pub fn parse_timestamp_or_relative_time_based(
    s: &str,
    base: OffsetDateTime,
    assume_negative_offset: bool,
) -> Result<OffsetDateTime, anyhow::Error> {
    if let Ok(t) = OffsetDateTime::parse(s, &Rfc3339) {
        return Ok(t);
    }
    if let Ok(t) = OffsetDateTime::parse(s, &Rfc2822) {
        return Ok(t);
    }
    if let Ok(t) = Date::parse(s, time::macros::format_description!("[year]-[month]-[day]")) {
        return Ok(PrimitiveDateTime::new(t, Time::MIDNIGHT).assume_utc());
    }
    if let Ok(t) = OffsetDateTime::parse(s, time::macros::format_description!("[unix_timestamp]")) {
        return Ok(t);
    }

    // Relative time.
    let (is_negative, v) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        // Note: `assume_negative_offset` forces offsets to be negative.
        None => (assume_negative_offset, s.trim_start_matches('+')),
    };

    if let Ok(duration) = humantime::parse_duration(v) {
        let time = if is_negative {
            base - duration
        } else {
            base + duration
        };

        return Ok(time);
    }

    anyhow::bail!("Unable to parse the timestamp - no known format matched")
}
