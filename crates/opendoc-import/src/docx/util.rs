//! Small shared helpers: element collection and ISO date parsing.

use crate::xml::XmlElement;

/// Collects `local`-named descendants, looking through content wrappers such as
/// `w:sdt`/`w:sdtContent`/`w:customXml` but not into nested tables.
pub(super) fn collect_wrapped<'a>(
    container: &'a XmlElement,
    local: &str,
    out: &mut Vec<&'a XmlElement>,
) {
    for element in container.elements() {
        if element.is(local) {
            out.push(element);
        } else if matches!(
            element.local.as_str(),
            "sdt" | "sdtContent" | "customXml" | "ins" | "del" | "moveTo" | "moveFrom"
        ) {
            collect_wrapped(element, local, out);
        }
    }
}

// ---------------------------------------------------------------------------
// Date parsing (ISO-8601 without external crates)
// ---------------------------------------------------------------------------

pub(super) fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = (year - era * 400) as u64;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index as u64 + 2) / 5 + day as u64 - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era as i64 - 719_468
}

pub(super) fn parse_iso_datetime_ms(value: &str) -> Option<u64> {
    let value = value.trim();
    let (date, time) = value.split_once('T')?;
    let mut date_parts = date.split('-');
    let year: i64 = date_parts.next()?.parse().ok()?;
    let month: u32 = date_parts.next()?.parse().ok()?;
    let day: u32 = date_parts.next()?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let clock_end = time.find(['Z', '+', '-']).unwrap_or(time.len());
    let (clock, zone) = time.split_at(clock_end);
    let (clock, fraction) = match clock.split_once('.') {
        Some((clock, fraction)) => (clock, Some(fraction)),
        None => (clock, None),
    };
    let mut clock_parts = clock.split(':');
    let hour: i64 = clock_parts.next()?.parse().ok()?;
    let minute: i64 = clock_parts.next()?.parse().ok()?;
    let second: i64 = clock_parts.next().unwrap_or("0").parse().ok()?;
    let offset_seconds: i64 = match zone {
        "" | "Z" => 0,
        signed => {
            let sign = if signed.starts_with('-') { -1 } else { 1 };
            let mut parts = signed[1..].split(':');
            let hours: i64 = parts.next()?.parse().ok()?;
            let minutes: i64 = parts.next().unwrap_or("0").parse().ok()?;
            sign * (hours * 3600 + minutes * 60)
        }
    };
    let seconds = days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second
        - offset_seconds;
    if seconds < 0 {
        return None;
    }
    let millis = fraction
        .map(|fraction| {
            let digits: String = fraction.chars().take(3).collect();
            let padded = format!("{digits:0<3}");
            padded.parse::<u64>().unwrap_or(0)
        })
        .unwrap_or(0);
    Some(seconds as u64 * 1000 + millis)
}
