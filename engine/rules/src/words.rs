//! Words cut to fit a line.

/// The start of `text` on one line: at most `most` characters, and
/// three dots where it was cut.
pub fn clip(text: &str, most: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match flat.char_indices().nth(most) {
        Some((cut, _)) => format!("{}...", &flat[..cut]),
        None => flat,
    }
}
