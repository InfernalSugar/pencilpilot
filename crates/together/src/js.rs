//! What the studio's TypeScript does with values, done the same way here: its number
//! checks, `String(…)`, truthiness, `trim`, string lengths in UTF-16 units, number
//! formatting (`JSON.stringify`) and its forgiving base64 (`atob`).
//!
//! Edits arrive as JSON from apps written in TypeScript; reading them as they do keeps
//! every app's copy the same.

use base64::Engine;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use serde_json::{Map, Value};

/// A JSON number for `v`, as JavaScript writes it: whole numbers without a fraction.
pub fn num(v: f64) -> Value {
    if v.is_finite() && v.fract() == 0.0 && v.abs() < 9.007_199_254_740_992e15 {
        if v == 0.0 {
            return Value::from(0);
        }
        return Value::from(v as i64);
    }
    serde_json::Number::from_f64(v).map_or(Value::Null, Value::Number)
}

/// `typeof v === 'number' && Number.isFinite(v)`.
pub fn finite(v: Option<&Value>) -> Option<f64> {
    v.and_then(Value::as_f64).filter(|x| x.is_finite())
}

/// `Number.isInteger(v)`.
pub fn integer(v: Option<&Value>) -> Option<f64> {
    finite(v).filter(|x| x.fract() == 0.0)
}

/// together.ts's `int`: a whole number under a million either way.
pub fn int6(v: Option<&Value>) -> Option<i64> {
    integer(v).filter(|x| x.abs() < 1e6).map(|x| x as i64)
}

/// JavaScript truthiness (`None`: undefined).
pub fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|x| x != 0.0 && !x.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// `String(v)` (`None`: undefined).
pub fn string(v: Option<&Value>) -> String {
    match v {
        None => "undefined".into(),
        Some(Value::Null) => "null".into(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => number_text(n.as_f64().unwrap_or(f64::NAN)),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) => a
            .iter()
            .map(|x| match x {
                Value::Null => String::new(),
                x => string(Some(x)),
            })
            .collect::<Vec<_>>()
            .join(","),
        Some(Value::Object(_)) => "[object Object]".into(),
    }
}

/// `v ?? fallback`: the value, unless it's null or missing.
pub fn or(v: Option<&Value>) -> Option<&Value> {
    v.filter(|x| !x.is_null())
}

/// `s.slice(0, n)`, counting UTF-16 units as JavaScript does. (A character cut in two
/// there, which JavaScript would keep half of, is left out: Rust strings hold whole
/// characters.)
pub fn slice16(s: &str, n: usize) -> String {
    let mut used = 0;
    let mut out = String::new();
    for c in s.chars() {
        used += c.len_utf16();
        if used > n {
            break;
        }
        out.push(c);
    }
    out
}

/// A string's `.length` (UTF-16 units).
pub fn len16(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

/// What JavaScript's `trim` takes off: white space and line ends (the BOM too).
fn js_space(c: char) -> bool {
    matches!(
        c,
        '\u{9}' | '\u{a}' | '\u{b}' | '\u{c}' | '\u{d}' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

/// `s.trim()`.
pub fn trim(s: &str) -> &str {
    s.trim_matches(js_space)
}

/// `+s`: a string as a number (NaN when it isn't one).
pub fn to_number(s: &str) -> f64 {
    let t = trim(s);
    if t.is_empty() {
        return 0.0;
    }
    let radix = |p: &str, r: u32| -> f64 {
        if p.is_empty() || !p.chars().all(|c| c.is_digit(r)) {
            return f64::NAN;
        }
        p.chars()
            .fold(0.0, |n, c| n * r as f64 + c.to_digit(r).unwrap() as f64)
    };
    let lower = t.to_ascii_lowercase();
    if let Some(p) = lower.strip_prefix("0x") {
        return radix(p, 16);
    }
    if let Some(p) = lower.strip_prefix("0o") {
        return radix(p, 8);
    }
    if let Some(p) = lower.strip_prefix("0b") {
        return radix(p, 2);
    }
    match t {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    // a decimal: digits, a point, digits, an exponent (no "inf", "nan", "_" as Rust takes)
    let b = t.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let mut digits = i > start;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        let f = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        digits |= i > f;
    }
    if !digits {
        return f64::NAN;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let e = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == e {
            return f64::NAN;
        }
    }
    if i != b.len() {
        return f64::NAN;
    }
    t.parse().unwrap_or(f64::NAN)
}

/// A number as JavaScript writes it (`String(n)`, and inside `JSON.stringify`, where
/// NaN and the infinities are `null`).
pub fn number_text(v: f64) -> String {
    if v.is_nan() {
        return "NaN".into();
    }
    if v.is_infinite() {
        return if v > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if v == 0.0 {
        return "0".into();
    }
    // Rust's shortest round-trip digits, laid out by ECMAScript's Number::toString rules
    let e = format!("{:e}", v.abs());
    let (mant, exp) = e.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    let n = exp + 1;
    let mut out = String::new();
    if v < 0.0 {
        out.push('-');
    }
    if k <= n && n <= 21 {
        out.push_str(&digits);
        out.extend(std::iter::repeat_n('0', (n - k) as usize));
    } else if 0 < n && n <= 21 {
        out.push_str(&digits[..n as usize]);
        out.push('.');
        out.push_str(&digits[n as usize..]);
    } else if -6 < n && n <= 0 {
        out.push_str("0.");
        out.extend(std::iter::repeat_n('0', (-n) as usize));
        out.push_str(&digits);
    } else {
        out.push_str(&digits[..1]);
        if k > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('e');
        out.push(if n > 0 { '+' } else { '-' });
        out.push_str(&(n - 1).abs().to_string());
    }
    out
}

/// `JSON.stringify(v)`: keys in the order the map keeps them, numbers as JavaScript
/// writes them.
pub fn stringify(v: &Value) -> String {
    let mut out = String::new();
    write_json(&mut out, v);
    out
}

fn write_json(out: &mut String, v: &Value) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            let x = n.as_f64().unwrap_or(f64::NAN);
            if x.is_finite() {
                out.push_str(&number_text(x))
            } else {
                out.push_str("null")
            }
        }
        Value::String(s) => out.push_str(&serde_json::to_string(s).unwrap()),
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_json(out, x);
            }
            out.push(']');
        }
        Value::Object(m) => write_object(out, m.iter()),
    }
}

/// An object's JSON, its entries in the order given.
pub fn write_object<'a>(out: &mut String, entries: impl Iterator<Item = (&'a String, &'a Value)>) {
    out.push('{');
    for (i, (k, x)) in entries.enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&serde_json::to_string(k).unwrap());
        out.push(':');
        write_json(out, x);
    }
    out.push('}');
}

/// An object with its entries in this order (for [`stringify`]-exact output).
pub fn ordered(entries: Vec<(&str, Value)>) -> Vec<(String, Value)> {
    entries
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect()
}

/// An object as a JSON value (key order is the map's own).
pub fn object(entries: Vec<(&str, Value)>) -> Value {
    let mut m = Map::new();
    for (k, v) in entries {
        m.insert(k.to_string(), v);
    }
    Value::Object(m)
}

const B64: GeneralPurpose = GeneralPurpose::new(
    &base64::alphabet::STANDARD,
    GeneralPurposeConfig::new()
        .with_encode_padding(true)
        .with_decode_padding_mode(DecodePaddingMode::Indifferent)
        .with_decode_allow_trailing_bits(true),
);

/// `btoa` of bytes (padded).
pub fn to_base64(b: &[u8]) -> String {
    B64.encode(b)
}

/// `atob` to bytes (white space ignored, padding optional); None where it throws.
pub fn from_base64(s: &str) -> Option<Vec<u8>> {
    let t: String = s
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\u{c}' | '\r' | ' '))
        .collect();
    let t = if t.len().is_multiple_of(4) {
        t.strip_suffix("==")
            .or_else(|| t.strip_suffix('='))
            .unwrap_or(&t)
    } else {
        &t
    };
    if t.len() % 4 == 1
        || !t
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'+' || c == b'/')
    {
        return None;
    }
    B64.decode(t).ok()
}

/// `^[0-9a-f]{12}:[0-9]{1,12}$`: a layer's, group's or drawing's shared id.
pub fn is_lid(s: &str) -> bool {
    let Some((m, n)) = s.split_once(':') else {
        return false;
    };
    m.len() == 12
        && m.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
        && (1..=12).contains(&n.len())
        && n.bytes().all(|c| c.is_ascii_digit())
}

/// `^[0-9a-f]{64}$`: a member's id.
pub fn is_member(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
}

/// A string value, if it is one and passes `ok`.
pub fn str_if(v: Option<&Value>, ok: impl Fn(&str) -> bool) -> Option<&str> {
    v.and_then(Value::as_str).filter(|s| ok(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_as_javascript_writes_them() {
        for (v, s) in [
            (1.0, "1"),
            (0.5, "0.5"),
            (-0.0, "0"),
            (1e21, "1e+21"),
            (1e20, "100000000000000000000"),
            (1.5e-7, "1.5e-7"),
            (0.000001, "0.000001"),
            (123.456, "123.456"),
            (0.1 + 0.2, "0.30000000000000004"),
            (f32::MAX as f64, "3.4028234663852886e+38"),
            (-2.5e-10, "-2.5e-10"),
        ] {
            assert_eq!(number_text(v), s);
        }
        assert_eq!(to_number(" 12 "), 12.0);
        assert_eq!(to_number(""), 0.0);
        assert_eq!(to_number("1e3"), 1000.0);
        assert_eq!(to_number("0x10"), 16.0);
        assert!(to_number("1_0").is_nan());
        assert!(to_number("inf").is_nan());
        assert_eq!(slice16("a😀b", 2), "a");
        assert_eq!(slice16("a😀b", 3), "a😀");
        assert_eq!(from_base64("AAA").unwrap(), vec![0, 0]);
        assert_eq!(from_base64("AAB="), Some(vec![0, 0]));
        assert!(from_base64("A").is_none());
        assert!(from_base64("A=A=").is_none());
    }
}
