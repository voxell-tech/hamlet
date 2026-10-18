//! Versions, and the ranges a plugin's `needs` name them in.

use std::fmt;

/// A version of three numbers. A suffix after `-` or `+` is ignored.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Version(pub u64, pub u64, pub u64);

impl Version {
    pub fn parse(text: &str) -> Option<Self> {
        let core = text.split(['-', '+']).next()?;
        let numbers = numbers(core)?;
        (numbers.len() == 3)
            .then(|| Self(numbers[0], numbers[1], numbers[2]))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

fn numbers(text: &str) -> Option<Vec<u64>> {
    let numbers = text
        .split('.')
        .map(|part| part.parse::<u64>().ok())
        .collect::<Option<Vec<_>>>()?;
    (1..=3).contains(&numbers.len()).then_some(numbers)
}

/// What a comparator compares with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Op {
    Ge,
    Gt,
    Le,
    Lt,
    Eq,
    Caret,
}

/// One comparison with a version of one to three numbers.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Comparator {
    op: Op,
    given: Vec<u64>,
}

impl Comparator {
    /// The version of the given numbers, the rest zero.
    fn floor(&self) -> Version {
        Version::of(&self.given)
    }

    /// The least version above everything the given numbers match,
    /// raising the number at `place`.
    fn above(&self, place: usize) -> Version {
        let mut raised = self.given[..=place].to_vec();
        raised[place] += 1;
        Version::of(&raised)
    }

    fn holds(&self, version: Version) -> bool {
        let last = self.given.len() - 1;
        match self.op {
            Op::Ge => version >= self.floor(),
            Op::Gt => version >= self.above(last),
            Op::Le => version < self.above(last),
            Op::Lt => version < self.floor(),
            Op::Eq => {
                version >= self.floor() && version < self.above(last)
            }
            Op::Caret => {
                let place = self
                    .given
                    .iter()
                    .position(|number| *number != 0)
                    .unwrap_or(last);
                version >= self.floor() && version < self.above(place)
            }
        }
    }
}

impl Version {
    fn of(given: &[u64]) -> Self {
        let at =
            |place: usize| given.get(place).copied().unwrap_or(0);
        Self(at(0), at(1), at(2))
    }
}

/// The versions a plugin needs of another: comparators that all hold,
/// separated by spaces or commas. `>=`, `>`, `<=`, `<`, `=` and `^`
/// compare with a version of one to three numbers; a bare version
/// is `^`. An empty range or `*` is any version.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Range(Vec<Comparator>);

impl Range {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut comparators = Vec::new();
        let mut words = text
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|word| !word.is_empty());
        while let Some(word) = words.next() {
            if word == "*" {
                continue;
            }
            let (op, rest) = split(word);
            let rest = match rest.is_empty() {
                true => words.next().unwrap_or(""),
                false => rest,
            };
            let given = numbers(rest).ok_or_else(|| {
                format!("`{text}` is not a version range")
            })?;
            comparators.push(Comparator { op, given });
        }
        Ok(Self(comparators))
    }

    pub fn holds(&self, version: Version) -> bool {
        self.0.iter().all(|comparator| comparator.holds(version))
    }
}

/// The operator a word starts with, and the rest.
fn split(word: &str) -> (Op, &str) {
    const OPS: [(&str, Op); 6] = [
        (">=", Op::Ge),
        ("<=", Op::Le),
        (">", Op::Gt),
        ("<", Op::Lt),
        ("=", Op::Eq),
        ("^", Op::Caret),
    ];
    OPS.iter()
        .find_map(|(text, op)| Some((*op, word.strip_prefix(text)?)))
        .unwrap_or((Op::Caret, word))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn holds(range: &str, version: &str) -> bool {
        Range::parse(range)
            .unwrap()
            .holds(Version::parse(version).unwrap())
    }

    #[test]
    fn a_range_holds_for_the_versions_it_names() {
        assert!(holds(">=0.2", "0.2.0") && !holds(">=0.2", "0.1.9"));
        assert!(holds(">0.2", "0.3.0") && !holds(">0.2", "0.2.7"));
        assert!(holds("<1", "0.9.9") && !holds("<1", "1.0.0"));
        assert!(holds("<=1.2", "1.2.9") && !holds("<=1.2", "1.3.0"));
        assert!(
            holds("=1.2.3", "1.2.3") && !holds("=1.2.3", "1.2.4")
        );
        assert!(holds("=1.2", "1.2.9") && !holds("=1.2", "1.3.0"));
        assert!(
            holds("^1.2.3", "1.9.0") && !holds("^1.2.3", "2.0.0")
        );
        assert!(
            holds("^0.2.3", "0.2.9") && !holds("^0.2.3", "0.3.0")
        );
        assert!(
            holds("^0.0.3", "0.0.3") && !holds("^0.0.3", "0.0.4")
        );
        assert!(holds("0.3", "0.3.5") && !holds("0.3", "0.4.0"));
        assert!(holds(">=0.2, <0.5", "0.4.0"));
        assert!(!holds(">=0.2 <0.5", "0.5.0"));
        assert!(holds(">= 0.2", "0.2.0"));
        assert!(holds("*", "9.9.9") && holds("", "0.0.1"));
        assert!(Range::parse(">=x").is_err());
        assert_eq!(
            Version::parse("1.2.3-beta"),
            Some(Version(1, 2, 3))
        );
        assert_eq!(Version::parse("1.2"), None);
    }
}
