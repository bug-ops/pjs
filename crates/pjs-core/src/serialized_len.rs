//! Allocation-free serialized length of JSON values, shared by the size gates and the planner.

use std::io;

use serde::Serialize;
use serde_json::Value;

/// Serialized byte length of `value`, without allocating the bytes.
pub(crate) fn json_len(value: &Value) -> usize {
    count(value)
}

/// Serialized byte length of `s` as a JSON string, without allocating the bytes.
#[cfg(any(test, feature = "websocket-client"))]
pub(crate) fn json_str_len(s: &str) -> usize {
    count(s)
}

fn count<T: Serialize + ?Sized>(value: &T) -> usize {
    let mut counter = Counter(0);
    // Serializing `Value` or `str` into an infallible writer cannot error.
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
