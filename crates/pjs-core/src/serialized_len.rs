//! Allocation-free serialized length of JSON values, shared by the size gates and the planner.

use std::io;

use serde_json::Value;

/// Serialized byte length of `value`, without allocating the bytes.
pub(crate) fn json_len(value: &Value) -> usize {
    count(value)
}

/// Serialized byte length of `s` as a JSON string, quotes included, without
/// allocating the bytes. Equals `serde_json`'s output length for every string.
#[cfg(any(test, feature = "websocket-client", feature = "websocket-server"))]
pub(crate) fn json_str_len(s: &str) -> usize {
    // Only ASCII bytes are ever escaped, and UTF-8 continuation bytes are >= 0x80.
    let extra = |byte: u8| match byte {
        b'"' | b'\\' | b'\n' | b'\r' | b'\t' | 0x08 | 0x0c => 1,
        0..0x20 => 5,
        _ => 0,
    };
    2 + s.len() + s.bytes().map(extra).sum::<usize>()
}

/// Number of base-10 digits in `n`'s decimal representation (`0` has 1 digit).
pub(crate) fn decimal_digits(n: u64) -> usize {
    n.checked_ilog10().map_or(1, |d| d as usize + 1)
}

fn count(value: &Value) -> usize {
    let mut counter = Counter(0);
    // Serializing a `Value` into an infallible writer cannot error.
    serde_json::to_writer(&mut counter, value).expect("counting writer is infallible");
    counter.0
}

struct Counter(usize);

impl io::Write for Counter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0 += buf.len();
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod acceptance;
