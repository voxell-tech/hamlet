//! The order a walk visits files in, on either side of the protocol.

use crate::{Found, MAX_FILE, Walked};

/// Most bytes of paths one walk gives.
pub const WALK_BYTES: usize = MAX_FILE;

/// An entry of a listed folder, as a walk sees it.
pub enum Listed {
    /// A regular file of this size.
    File(u64),
    Directory,
    Other,
}

/// The regular files under `root`, depth first, with each folder's
/// files in name order before its subfolders. It passes over folders
/// named in `skip` and folders `list` cannot read, and stops after
/// `files` files or [`WALK_BYTES`] of paths.
pub fn walk<E>(
    root: &str,
    skip: &[String],
    files: u64,
    mut list: impl FnMut(&str) -> Result<Vec<(String, Listed)>, E>,
) -> Walked {
    let mut todo = vec![root.to_owned()];
    let mut pending = Vec::<Found>::new();
    let mut walked = Walked::default();
    let mut bytes = 0;
    loop {
        if let Some(found) = pending.pop() {
            bytes += found.path.len();
            if walked.files.len() as u64 >= files
                || bytes > WALK_BYTES
            {
                walked.capped = true;
                return walked;
            }
            walked.files.push(found);
            continue;
        }
        let Some(dir) = todo.pop() else {
            return walked;
        };
        let Ok(mut listed) = list(&dir) else {
            continue;
        };
        listed.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, entry) in listed.into_iter().rev() {
            let path = if dir.ends_with('/') {
                format!("{dir}{name}")
            } else {
                format!("{dir}/{name}")
            };
            match entry {
                Listed::Directory if !skip.contains(&name) => {
                    todo.push(path);
                }
                Listed::File(size) => {
                    pending.push(Found { path, size });
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listing(dir: &str) -> Result<Vec<(String, Listed)>, ()> {
        let entries = match dir {
            "/r" => vec![
                ("b", Listed::Directory),
                ("z.txt", Listed::File(2)),
                ("a", Listed::Directory),
                ("target", Listed::Directory),
                ("link", Listed::Other),
                ("m.txt", Listed::File(1)),
            ],
            "/r/a" => vec![("deep", Listed::Directory)],
            "/r/a/deep" => vec![("x", Listed::File(3))],
            "/r/b" => vec![("y", Listed::File(4))],
            "/r/target" => vec![("hidden", Listed::File(5))],
            _ => return Err(()),
        };
        Ok(entries
            .into_iter()
            .map(|(name, kind)| (name.to_owned(), kind))
            .collect())
    }

    #[test]
    fn gives_a_folders_files_before_its_subfolders_in_name_order() {
        let walked = walk("/r", &["target".into()], 10, listing);
        assert_eq!(
            walked
                .files
                .iter()
                .map(|found| found.path.as_str())
                .collect::<Vec<_>>(),
            ["/r/m.txt", "/r/z.txt", "/r/a/deep/x", "/r/b/y"]
        );
        assert!(!walked.capped);
        let capped = walk("/r", &[], 3, listing);
        assert_eq!(capped.files.len(), 3);
        assert!(capped.capped);
        assert!(!walk("/r", &[], 5, listing).capped);
    }
}
