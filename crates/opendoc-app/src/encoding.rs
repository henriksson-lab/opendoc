//! Small encoding helpers shared by file import and render projections.

pub fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[(triple >> 18) as usize & 63] as char);
        out.push(TABLE[(triple >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(TABLE[(triple >> 6) as usize & 63] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[triple as usize & 63] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// Decode standard base64 (with or without padding); `None` on bad input.
pub fn base64_decode(value: &str) -> Option<Vec<u8>> {
    fn digit(ch: u8) -> Option<u32> {
        match ch {
            b'A'..=b'Z' => Some((ch - b'A') as u32),
            b'a'..=b'z' => Some((ch - b'a') as u32 + 26),
            b'0'..=b'9' => Some((ch - b'0') as u32 + 52),
            b'+' | b'-' => Some(62),
            b'/' | b'_' => Some(63),
            _ => None,
        }
    }
    let mut out = Vec::with_capacity(value.len() * 3 / 4);
    let mut buffer = 0u32;
    let mut bits = 0u32;
    for &ch in value.as_bytes() {
        if ch == b'=' || ch == b'\n' || ch == b'\r' || ch == b' ' {
            continue;
        }
        buffer = (buffer << 6) | digit(ch)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buffer >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips() {
        let sample = b"hello world";
        assert_eq!(base64_decode(&base64_encode(sample)).unwrap(), sample);
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        assert!(base64_decode("!!").is_none());
    }
}
