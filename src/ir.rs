//! Conversion from `serde_json::Value` into native PHP values (`Zval`).
//!
//! `office_oxide`'s intermediate representation (`DocumentIR`) is a deep tree of
//! `serde`-serializable structs. Rather than hand-mapping every node type to a
//! dedicated PHP class, we serialize the IR to `serde_json::Value` once and walk
//! that value, building an equivalent tree of PHP arrays and scalars. This keeps
//! the whole structured surface available to PHP with a single conversion path.

use ext_php_rs::convert::IntoZval;
use ext_php_rs::types::{ZendHashTable, Zval};
use serde_json::Value;

/// Maximum nesting depth converted from the IR into PHP values.
///
/// The IR is derived from untrusted documents, and this conversion recurses on
/// the PHP request thread's stack — unlike `office_oxide`'s *parsing*, which
/// runs on a dedicated large-stack thread. Capping the depth turns a potential
/// stack-overflow crash (an uncatchable `SIGSEGV` that takes down the PHP
/// worker) on pathologically nested input into a bounded truncation. Real
/// Office documents nest far shallower than this.
const MAX_DEPTH: usize = 256;

/// Recursively convert a `serde_json::Value` into a PHP `Zval`.
///
/// Objects and arrays become PHP arrays (associative and list-style
/// respectively), mirroring how `json_decode($json, true)` would shape the data.
///
/// Nesting deeper than [`MAX_DEPTH`] is truncated to `null` — see that constant.
pub fn json_to_zval(value: &Value) -> Zval {
    json_to_zval_at(value, 0)
}

/// A PHP `null` value.
fn null_zval() -> Zval {
    let mut z = Zval::new();
    z.set_null();
    z
}

fn json_to_zval_at(value: &Value, depth: usize) -> Zval {
    if depth >= MAX_DEPTH {
        return null_zval();
    }
    match value {
        Value::Null => null_zval(),
        Value::Bool(b) => {
            let mut z = Zval::new();
            z.set_bool(*b);
            z
        }
        Value::Number(n) => number_to_zval(n),
        Value::String(s) => {
            let mut z = Zval::new();
            // Not persistent: the string lives for the duration of the request.
            // `set_string` can only fail on allocation failure, which PHP treats
            // as fatal anyway; fall back to null so we never return an undefined
            // zval.
            if z.set_string(s, false).is_err() {
                z.set_null();
            }
            z
        }
        Value::Array(items) => {
            let mut ht = ZendHashTable::new();
            for item in items {
                // A push to a fresh list-array cannot fail in practice; assert
                // it in debug builds rather than silently dropping an element.
                if ht.push(json_to_zval_at(item, depth + 1)).is_err() {
                    debug_assert!(false, "pushing to a fresh PHP list cannot fail");
                }
            }
            // A freshly built hashtable always converts into a Zval.
            ht.into_zval(false)
                .expect("a freshly built hashtable always converts into a Zval")
        }
        Value::Object(map) => {
            let mut ht = ZendHashTable::new();
            for (key, val) in map {
                if ht
                    .insert(key.as_str(), json_to_zval_at(val, depth + 1))
                    .is_err()
                {
                    debug_assert!(false, "inserting into a fresh PHP array cannot fail");
                }
            }
            ht.into_zval(false)
                .expect("a freshly built hashtable always converts into a Zval")
        }
    }
}

/// Convert a JSON number into a PHP int or float.
///
/// PHP integers are signed 64-bit. Values that fit are represented as `long`;
/// anything larger (or fractional) falls back to `double` to avoid overflow.
/// Note that a `u64` above `i64::MAX` is widened to `f64` and so loses integer
/// precision — PHP has no unsigned 64-bit integer, and this mirrors what
/// `json_decode` does with the same input.
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
        // Unreachable unless serde_json's `arbitrary_precision` feature is
        // enabled (it is not here), which can hold a number that is none of
        // i64/u64/f64. Fall back to null rather than panicking.
        z.set_null();
    }
    z
}
