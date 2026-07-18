//! Conversion from `serde_json::Value` into native PHP values (`Zval`).
//!
//! `office_oxide`'s intermediate representation (`DocumentIR`) is a deep tree of
//! `serde`-serializable structs. Rather than hand-mapping every node type to a
//! dedicated PHP class, we serialize the IR to `serde_json::Value` once and walk
//! that value, building an equivalent tree of PHP arrays and scalars. This keeps
//! the whole structured surface available to PHP with a single conversion path.

use ext_php_rs::convert::IntoZval;
use ext_php_rs::types::{Zval, ZendHashTable};
use serde_json::Value;

/// Recursively convert a `serde_json::Value` into a PHP `Zval`.
///
/// Objects and arrays become PHP arrays (associative and list-style
/// respectively), mirroring how `json_decode($json, true)` would shape the data.
pub fn json_to_zval(value: &Value) -> Zval {
    match value {
        Value::Null => {
            let mut z = Zval::new();
            z.set_null();
            z
        }
        Value::Bool(b) => {
            let mut z = Zval::new();
            z.set_bool(*b);
            z
        }
        Value::Number(n) => number_to_zval(n),
        Value::String(s) => {
            let mut z = Zval::new();
            // Not persistent: the string lives for the duration of the request.
            let _ = z.set_string(s, false);
            z
        }
        Value::Array(items) => {
            let mut ht = ZendHashTable::new();
            for item in items {
                let _ = ht.push(json_to_zval(item));
            }
            // A freshly built hashtable always converts cleanly into a Zval.
            ht.into_zval(false).unwrap_or_else(|_| {
                let mut z = Zval::new();
                z.set_null();
                z
            })
        }
        Value::Object(map) => {
            let mut ht = ZendHashTable::new();
            for (key, val) in map {
                let _ = ht.insert(key.as_str(), json_to_zval(val));
            }
            ht.into_zval(false).unwrap_or_else(|_| {
                let mut z = Zval::new();
                z.set_null();
                z
            })
        }
    }
}

/// Convert a JSON number into a PHP int or float.
///
/// PHP integers are signed 64-bit. Values that fit are represented as `long`;
/// anything larger (or fractional) falls back to `double` to avoid overflow.
fn number_to_zval(n: &serde_json::Number) -> Zval {
    let mut z = Zval::new();
    if let Some(i) = n.as_i64() {
        z.set_long(i);
    } else if let Some(u) = n.as_u64() {
        if u <= i64::MAX as u64 {
            z.set_long(u as i64);
        } else {
            z.set_double(u as f64);
        }
    } else if let Some(f) = n.as_f64() {
        z.set_double(f);
    } else {
        z.set_null();
    }
    z
}
