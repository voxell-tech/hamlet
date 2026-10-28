//! The rules of a draft's tags: each lowercase, its spaces each a
//! `-`, at most eight, and the tags in use offered as one is typed.

use crate::bazaar::market::versions::TagCount;

/// The most tags a version has.
pub(crate) const MOST_TAGS: usize = 8;

/// The most letters of a tag.
const MOST_LETTERS: usize = 24;

/// The most tags in use offered under the field.
const OFFERED: usize = 5;

/// A tag offered under the field.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum Offered {
    /// In use, by this many listings.
    Used(String, u32),
    New(String),
}

impl Offered {
    pub fn tag(&self) -> &str {
        match self {
            Offered::Used(tag, _) | Offered::New(tag) => tag,
        }
    }
}

/// Why a tag was not added.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Refused {
    Full,
    Empty,
    Had,
}

/// `typed` as a tag: lowercase, its spaces each a `-`.
pub(crate) fn tag_of(typed: &str) -> Option<String> {
    let tag = typed
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
        .to_lowercase()
        .chars()
        .take(MOST_LETTERS)
        .collect::<String>();
    (!tag.is_empty()).then_some(tag)
}

/// Adds `typed` to `tags` as a tag.
pub(crate) fn add(
    tags: &mut Vec<String>,
    typed: &str,
) -> Result<(), Refused> {
    let tag = tag_of(typed).ok_or(Refused::Empty)?;
    if tags.contains(&tag) {
        return Err(Refused::Had);
    }
    if tags.len() >= MOST_TAGS {
        return Err(Refused::Full);
    }
    tags.push(tag);
    Ok(())
}

/// The tags offered for `typed`: those in use that hold it, most used
/// first, then `typed` as a new tag unless it is one of them.
pub(crate) fn offered(
    typed: &str,
    used: &[TagCount],
    tags: &[String],
) -> Vec<Offered> {
    let Some(typed) = tag_of(typed) else {
        return Vec::new();
    };
    let mut matching = used
        .iter()
        .filter(|one| {
            one.tag.contains(&typed) && !tags.contains(&one.tag)
        })
        .collect::<Vec<_>>();
    matching.sort_by(|a, b| {
        b.count.cmp(&a.count).then(a.tag.cmp(&b.tag))
    });
    let mut all = matching
        .into_iter()
        .take(OFFERED)
        .map(|one| Offered::Used(one.tag.clone(), one.count))
        .collect::<Vec<_>>();
    let known = used.iter().any(|one| one.tag == typed);
    if !known && !tags.contains(&typed) {
        all.push(Offered::New(typed));
    }
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tag_is_lowercase_with_dashes_and_there_are_at_most_eight() {
        let mut tags = Vec::new();
        assert_eq!(add(&mut tags, "  Coastal   Town "), Ok(()));
        assert_eq!(tags, ["coastal-town"]);
        assert_eq!(add(&mut tags, "coastal town"), Err(Refused::Had));
        assert_eq!(add(&mut tags, "   "), Err(Refused::Empty));
        for at in 0..7 {
            assert_eq!(add(&mut tags, &format!("t{at}")), Ok(()));
        }
        assert_eq!(add(&mut tags, "ninth"), Err(Refused::Full));
        assert_eq!(tags.len(), MOST_TAGS);
    }

    #[test]
    fn the_tags_in_use_are_offered_most_used_first_then_a_new_one() {
        let used = [
            ("coat-of-arms", 3),
            ("coast", 48),
            ("coastal-town", 12),
        ]
        .map(|(tag, count)| TagCount {
            tag: tag.into(),
            count,
        });
        let had = vec!["coastal-town".to_string()];
        assert_eq!(
            offered("Coa", &used, &had),
            [
                Offered::Used("coast".into(), 48),
                Offered::Used("coat-of-arms".into(), 3),
                Offered::New("coa".into()),
            ]
        );
        assert_eq!(
            offered("coast", &used, &had),
            [Offered::Used("coast".into(), 48)]
        );
        assert!(offered(" ", &used, &had).is_empty());
    }
}
