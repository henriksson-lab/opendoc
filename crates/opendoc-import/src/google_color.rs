//! Google Docs RGB colour conversion, in both directions.

use serde_json::{json, Value};

pub(crate) fn import_google_rgb(value: &Value) -> Option<String> {
    let red = value.get("red").and_then(Value::as_f64).unwrap_or(0.0);
    let green = value.get("green").and_then(Value::as_f64).unwrap_or(0.0);
    let blue = value.get("blue").and_then(Value::as_f64).unwrap_or(0.0);
    Some(format!(
        "#{:02x}{:02x}{:02x}",
        float_color(red),
        float_color(green),
        float_color(blue)
    ))
}

pub(crate) fn export_google_color(value: &str) -> Value {
    let value = value.trim_start_matches('#');
    let red = u8::from_str_radix(value.get(0..2).unwrap_or("00"), 16).unwrap_or(0);
    let green = u8::from_str_radix(value.get(2..4).unwrap_or("00"), 16).unwrap_or(0);
    let blue = u8::from_str_radix(value.get(4..6).unwrap_or("00"), 16).unwrap_or(0);
    json!({
        "color": {
            "rgbColor": {
                "red": red as f64 / 255.0,
                "green": green as f64 / 255.0,
                "blue": blue as f64 / 255.0,
            }
        }
    })
}

pub(crate) fn float_color(value: f64) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

pub(crate) fn trim_float(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        value.to_string()
    }
}
