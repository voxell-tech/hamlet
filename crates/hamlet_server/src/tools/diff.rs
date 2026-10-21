//! The diff and the patch of an edit, as pi's `edit` makes them.

use std::collections::HashMap;

/// A run of lines both texts have, or one of them alone.
struct Part {
    value: String,
    added: bool,
    removed: bool,
}

/// One link of a path's chain of runs.
struct Node {
    count: usize,
    added: bool,
    removed: bool,
    previous: Option<usize>,
}

#[derive(Clone, Copy)]
struct Track {
    old_pos: isize,
    last: Option<usize>,
}

/// The lines of `text`, each with its newline.
fn tokens(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

/// The line diff of pi's `edit`: Myers's algorithm with its pruning
/// of the diagonals past an edge.
fn diff_lines(old: &str, new: &str) -> Vec<Part> {
    let old_tokens = tokens(old);
    let new_tokens = tokens(new);
    let (old_len, new_len) =
        (old_tokens.len() as isize, new_tokens.len() as isize);
    let mut nodes = Vec::<Node>::new();
    let common = |track: &mut Track,
                  diagonal: isize,
                  nodes: &mut Vec<Node>| {
        let mut old_pos = track.old_pos;
        let mut new_pos = old_pos - diagonal;
        let mut count = 0;
        while new_pos + 1 < new_len
            && old_pos + 1 < old_len
            && old_tokens[(old_pos + 1) as usize]
                == new_tokens[(new_pos + 1) as usize]
        {
            new_pos += 1;
            old_pos += 1;
            count += 1;
        }
        if count > 0 {
            nodes.push(Node {
                count,
                added: false,
                removed: false,
                previous: track.last,
            });
            track.last = Some(nodes.len() - 1);
        }
        track.old_pos = old_pos;
        new_pos
    };
    let extend = |track: Track,
                  added: bool,
                  nodes: &mut Vec<Node>| {
        let removed = !added;
        let node = match track.last.map(|last| &nodes[last]) {
            Some(last)
                if last.added == added && last.removed == removed =>
            {
                Node {
                    count: last.count + 1,
                    added,
                    removed,
                    previous: last.previous,
                }
            }
            _ => Node {
                count: 1,
                added,
                removed,
                previous: track.last,
            },
        };
        nodes.push(node);
        Track {
            old_pos: track.old_pos + isize::from(removed),
            last: Some(nodes.len() - 1),
        }
    };
    let mut first = Track {
        old_pos: -1,
        last: None,
    };
    let new_pos = common(&mut first, 0, &mut nodes);
    let mut end = None;
    if first.old_pos + 1 >= old_len && new_pos + 1 >= new_len {
        end = Some(first.last);
    }
    let mut best = HashMap::from([(0isize, first)]);
    let (mut min_diagonal, mut max_diagonal) =
        (isize::MIN, isize::MAX);
    let mut length = 1;
    'search: while end.is_none() && length <= old_len + new_len {
        let mut diagonal = min_diagonal.max(-length);
        while diagonal <= max_diagonal.min(length) {
            let remove = best.remove(&(diagonal - 1));
            let add = best.get(&(diagonal + 1)).copied();
            let can_add = add.is_some_and(|add| {
                (0..new_len).contains(&(add.old_pos - diagonal))
            });
            let can_remove = remove
                .is_some_and(|remove| remove.old_pos + 1 < old_len);
            let base = match (add, remove) {
                (Some(add), remove)
                    if can_add
                        && (!can_remove
                            || remove.is_some_and(|r| {
                                r.old_pos < add.old_pos
                            })) =>
                {
                    Some(extend(add, true, &mut nodes))
                }
                (_, Some(remove)) if can_remove => {
                    Some(extend(remove, false, &mut nodes))
                }
                _ => None,
            };
            let Some(mut base) = base else {
                best.remove(&diagonal);
                diagonal += 2;
                continue;
            };
            let new_pos = common(&mut base, diagonal, &mut nodes);
            if base.old_pos + 1 >= old_len && new_pos + 1 >= new_len {
                end = Some(base.last);
                break 'search;
            }
            best.insert(diagonal, base);
            if base.old_pos + 1 >= old_len {
                max_diagonal = max_diagonal.min(diagonal - 1);
            }
            if new_pos + 1 >= new_len {
                min_diagonal = min_diagonal.max(diagonal + 1);
            }
            diagonal += 2;
        }
        length += 1;
    }
    let mut chain = Vec::new();
    let mut at = end.flatten();
    while let Some(index) = at {
        chain.push(index);
        at = nodes[index].previous;
    }
    let (mut old_pos, mut new_pos) = (0, 0);
    chain
        .into_iter()
        .rev()
        .map(|index| {
            let node = &nodes[index];
            let value = if node.removed {
                let value = old_tokens[old_pos..old_pos + node.count]
                    .concat();
                old_pos += node.count;
                value
            } else {
                let value = new_tokens[new_pos..new_pos + node.count]
                    .concat();
                new_pos += node.count;
                if !node.added {
                    old_pos += node.count;
                }
                value
            };
            Part {
                value,
                added: node.added,
                removed: node.removed,
            }
        })
        .collect()
}

/// pi's display diff: changed lines numbered, with a few lines around
/// them, and the first changed line of the new text.
pub(crate) fn diff_string(
    old: &str,
    new: &str,
) -> (String, Option<usize>) {
    const CONTEXT: usize = 4;
    let parts = diff_lines(old, new);
    let longest =
        old.split('\n').count().max(new.split('\n').count());
    let mut out = Numbered {
        width: longest.to_string().len(),
        old: 1,
        new: 1,
        lines: Vec::new(),
    };
    let mut last_was_change = false;
    let mut first_changed = None;
    for (i, part) in parts.iter().enumerate() {
        let mut raw = part.value.split('\n').collect::<Vec<_>>();
        if raw.last() == Some(&"") {
            raw.pop();
        }
        if part.added || part.removed {
            first_changed.get_or_insert(out.new);
            for line in raw {
                let (mark, at) = if part.added {
                    out.new += 1;
                    ('+', out.new - 1)
                } else {
                    out.old += 1;
                    ('-', out.old - 1)
                };
                out.lines
                    .push(format!("{mark}{} {line}", out.number(at)));
            }
            last_was_change = true;
            continue;
        }
        let next_is_change = parts
            .get(i + 1)
            .is_some_and(|next| next.added || next.removed);
        match (last_was_change, next_is_change) {
            (true, true) if raw.len() <= CONTEXT * 2 => {
                out.context(&raw)
            }
            (true, true) => {
                out.context(&raw[..CONTEXT]);
                out.skip(raw.len() - 2 * CONTEXT);
                out.context(&raw[raw.len() - CONTEXT..]);
            }
            (true, false) => {
                let shown = raw.len().min(CONTEXT);
                out.context(&raw[..shown]);
                if raw.len() > shown {
                    out.skip(raw.len() - shown);
                }
            }
            (false, true) => {
                let skipped = raw.len().saturating_sub(CONTEXT);
                if skipped > 0 {
                    out.skip(skipped);
                }
                out.context(&raw[skipped..]);
            }
            (false, false) => {
                out.old += raw.len();
                out.new += raw.len();
            }
        }
        last_was_change = false;
    }
    (out.lines.join("\n"), first_changed)
}

/// The numbered lines of a display diff, and the next line of each
/// text.
struct Numbered {
    width: usize,
    old: usize,
    new: usize,
    lines: Vec<String>,
}

impl Numbered {
    fn number(&self, n: usize) -> String {
        format!("{n:>width$}", width = self.width)
    }

    fn context(&mut self, lines: &[&str]) {
        for line in lines {
            self.lines
                .push(format!(" {} {line}", self.number(self.old)));
            self.old += 1;
            self.new += 1;
        }
    }

    fn skip(&mut self, lines: usize) {
        self.lines.push(format!(" {} ...", " ".repeat(self.width)));
        self.old += lines;
        self.new += lines;
    }
}

/// Lines of `text`, each with its newline but the last.
fn split_lines(text: &str) -> Vec<String> {
    let mut lines = text
        .split('\n')
        .map(|line| format!("{line}\n"))
        .collect::<Vec<_>>();
    if text.ends_with('\n') {
        lines.pop();
    } else if let Some(last) = lines.last_mut() {
        last.pop();
    }
    lines
}

/// pi's unified patch of `path`, with file headers only and four
/// lines of context.
pub(crate) fn unified_patch(
    path: &str,
    old: &str,
    new: &str,
) -> String {
    const CONTEXT: usize = 4;
    let mut diff = diff_lines(old, new)
        .into_iter()
        .map(|part| {
            (split_lines(&part.value), part.added, part.removed)
        })
        .collect::<Vec<_>>();
    diff.push((Vec::new(), false, false));
    struct Hunk {
        old_start: usize,
        old_lines: usize,
        new_start: usize,
        new_lines: usize,
        lines: Vec<String>,
    }
    let mut hunks = Vec::<Hunk>::new();
    let (mut old_start, mut new_start) = (0, 0);
    let mut range = Vec::<String>::new();
    let (mut old_line, mut new_line) = (1, 1);
    let context = |lines: &[String]| {
        lines
            .iter()
            .map(|line| format!(" {line}"))
            .collect::<Vec<_>>()
    };
    for i in 0..diff.len() {
        let (lines, added, removed) = &diff[i];
        if *added || *removed {
            if old_start == 0 {
                old_start = old_line;
                new_start = new_line;
                if i > 0 {
                    let previous = &diff[i - 1].0;
                    range = context(
                        &previous[previous
                            .len()
                            .saturating_sub(CONTEXT)..],
                    );
                    old_start -= range.len();
                    new_start -= range.len();
                }
            }
            let mark = if *added { '+' } else { '-' };
            range.extend(
                lines.iter().map(|line| format!("{mark}{line}")),
            );
            if *added {
                new_line += lines.len();
            } else {
                old_line += lines.len();
            }
            continue;
        }
        if old_start != 0 {
            if lines.len() <= CONTEXT * 2 && i + 2 < diff.len() {
                range.extend(context(lines));
            } else {
                let size = lines.len().min(CONTEXT);
                range.extend(context(&lines[..size]));
                hunks.push(Hunk {
                    old_start,
                    old_lines: old_line - old_start + size,
                    new_start,
                    new_lines: new_line - new_start + size,
                    lines: std::mem::take(&mut range),
                });
                old_start = 0;
                new_start = 0;
            }
        }
        old_line += lines.len();
        new_line += lines.len();
    }
    let mut out = vec![format!("--- {path}"), format!("+++ {path}")];
    for hunk in &mut hunks {
        let mut lines = Vec::new();
        for line in hunk.lines.drain(..) {
            match line.strip_suffix('\n') {
                Some(line) => lines.push(line.to_owned()),
                None => {
                    lines.push(line);
                    lines.push("\\ No newline at end of file".into());
                }
            }
        }
        // A hunk of no lines starts one before.
        let old_start =
            hunk.old_start - usize::from(hunk.old_lines == 0);
        let new_start =
            hunk.new_start - usize::from(hunk.new_lines == 0);
        out.push(format!(
            "@@ -{old_start},{} +{new_start},{} @@",
            hunk.old_lines, hunk.new_lines
        ));
        out.extend(lines);
    }
    out.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_the_changed_lines_and_patches_them_as_pi_does() {
        let old = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\n";
        let new = "a\nb\nc\nd\ne\nF\ng\nh\ni\nj\nk";
        let (diff, first) = diff_string(old, new);
        assert_eq!(first, Some(6));
        assert_eq!(
            diff,
            "    ...\n  2 b\n  3 c\n  4 d\n  5 e\n- 6 f\n+ 6 F\n  7 \
             g\n  8 h\n  9 i\n 10 j\n-11 k\n+11 k"
        );
        assert_eq!(
            unified_patch("p", old, new),
            "--- p\n+++ p\n@@ -2,10 +2,10 @@\n b\n c\n d\n \
             e\n-f\n+F\n g\n h\n i\n j\n-k\n+k\n\\ No newline at \
             end of file\n"
        );
        let numbers = (1..=10)
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let old = format!("x\ny\nz\nq\nw\n{numbers}\nlast\n");
        let new = format!("y\nx\nz\nw\nq\n{numbers}\nLAST\nmore\n");
        let (old, new) = (old.as_str(), new.as_str());
        assert_eq!(
            diff_string(old, new).0,
            "- 1 x\n  2 y\n+ 2 x\n  3 z\n- 4 q\n  5 w\n+ 5 q\n  6 \
             1\n  7 2\n  8 3\n  9 4\n    ...\n 12 7\n 13 8\n 14 9\n \
             15 10\n-16 last\n+16 LAST\n+17 more"
        );
        assert_eq!(
            unified_patch("q", old, new),
            "--- q\n+++ q\n@@ -1,9 +1,9 @@\n-x\n y\n+x\n z\n-q\n \
             w\n+q\n 1\n 2\n 3\n 4\n@@ -12,5 +12,6 @@\n 7\n 8\n 9\n \
             10\n-last\n+LAST\n+more\n"
        );
    }
}
