//! The shape of a tool call's arguments, so that the same command
//! with other literals is told as a repeat.

use serde_json::Value;

/// Normalized shape of tool arguments for repeat detection: same
/// command with different literals maps to the same template.
pub(super) fn args_template(raw_args: &str) -> String {
    match serde_json::from_str::<Value>(raw_args) {
        Ok(mut v) => {
            normalize_value(&mut v);
            // Map is a BTreeMap here, so keys serialize sorted.
            v.to_string()
        }
        Err(_) => normalize_str(raw_args),
    }
}

fn normalize_value(v: &mut Value) {
    match v {
        Value::String(s) => *s = normalize_str(s),
        Value::Array(items) => {
            items.iter_mut().for_each(normalize_value)
        }
        Value::Object(map) => {
            map.values_mut().for_each(normalize_value)
        }
        _ => {}
    }
}

fn normalize_str(s: &str) -> String {
    let unquoted = replace_quoted(s);
    unquoted
        .split_whitespace()
        .map(|w| {
            if w.contains('/') {
                "PATH".to_string()
            } else {
                replace_digits(w)
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Replaces '...' and "..." spans with `'_'`. An unmatched quote is
/// kept as is.
fn replace_quoted(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.char_indices();
    while let Some((i, c)) = chars.next() {
        if (c == '\'' || c == '"')
            && let Some(end) = s[i + 1..].find(c)
        {
            out.push_str("'_'");
            let close = i + 1 + end;
            while chars.next().is_some_and(|(j, _)| j < close) {}
            continue;
        }
        out.push(c);
    }
    out
}

fn replace_digits(s: &str) -> String {
    let mut out = String::new();
    let mut in_digits = false;
    for c in s.chars() {
        if c.is_ascii_digit() {
            if !in_digits {
                out.push('0');
            }
            in_digits = true;
        } else {
            in_digits = false;
            out.push(c);
        }
    }
    out
}
