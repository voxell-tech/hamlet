//! skills.sh, which lists the skills kept in GitHub repositories by
//! how often they were installed, and gives a skill's files.

use std::path::{Component, Path};

use serde_json::Value;

/// The address skills.sh answers at.
const SITE: &str = "https://skills.sh";

/// The pages of the leaderboard asked for, of 200 skills each.
pub(crate) const PAGES: u32 = 3;

/// The skills on a page of the leaderboard.
const PER_PAGE: u32 = 200;

/// The most skills a search asks for.
const FOUND_MOST: usize = 50;

/// The most bytes of one file of a skill kept.
const FILE_MOST: usize = 512 * 1024;

/// The most bytes of all of a skill's files kept.
const SKILL_MOST: usize = 2 * 1024 * 1024;

/// The most files of a skill kept.
const FILES_MOST: usize = 100;

/// The longest name of a skill, a repository or its owner.
const NAME_MOST: usize = 100;

/// The deepest a skill's file may lie in its folder.
const DEPTH_MOST: usize = 8;

/// One skill skills.sh lists.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub(crate) struct Scroll {
    /// The GitHub repository it is kept in, as `owner/repo`.
    pub source: String,
    /// skills.sh's name for it in that repository, and its folder's
    /// name once taken.
    pub skill: String,
    /// The name it is shown by.
    pub name: String,
    pub installs: u64,
    /// Its place on the leaderboard, from 1. None for one found by a
    /// search.
    pub rank: Option<u32>,
    /// Kept by the makers of what it is about.
    pub official: bool,
}

impl Scroll {
    /// skills.sh's id of it, `owner/repo/skill`.
    pub fn path(&self) -> String {
        format!("{}/{}", self.source, self.skill)
    }

    /// The repository it is kept in, on GitHub.
    pub fn repository(&self) -> String {
        format!("https://github.com/{}", self.source)
    }
}

/// The address of the page `page` of the leaderboard of all time.
pub(crate) fn board(page: u32) -> String {
    format!("{SITE}/api/skills/all-time/{page}")
}

/// The address of a search for `words`.
pub(crate) fn search(words: &str) -> String {
    format!(
        "{SITE}/api/search?q={}&limit={FOUND_MOST}",
        escaped(words.trim())
    )
}

/// The address of the files of the skill `path`, `owner/repo/skill`.
pub(crate) fn download(path: &str) -> String {
    format!("{SITE}/api/download/{path}")
}

/// `words` as a value of an address's query.
fn escaped(words: &str) -> String {
    words
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'~' => char::from(byte).to_string(),
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

/// True when `name` can be a folder's name: letters, digits, `.`,
/// `_` and `-`, beginning with a letter or a digit.
pub(crate) fn safe_name(name: &str) -> bool {
    name.len() <= NAME_MOST
        && name.starts_with(|c: char| c.is_ascii_alphanumeric())
        && name.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')
        })
}

/// True when `source` is a GitHub repository, as `owner/repo`.
fn safe_source(source: &str) -> bool {
    source.split_once('/').is_some_and(|(owner, repo)| {
        safe_name(owner) && safe_name(repo)
    })
}

/// True when `path` stays inside the folder it is read from: plain
/// names joined by `/`, none of them `.` or `..`.
fn safe_path(path: &str) -> bool {
    let parts = path.split('/').collect::<Vec<_>>();
    !path.is_empty()
        && parts.len() <= DEPTH_MOST
        && parts.iter().all(|part| {
            !part.is_empty()
                && !matches!(*part, "." | "..")
                && part.len() <= NAME_MOST
                && !part.chars().any(|c| {
                    c.is_control() || matches!(c, '\\' | ':')
                })
        })
        && Path::new(path)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

/// The skills in skills.sh's answer of a page of its leaderboard, or
/// of a search for none. Those kept anywhere but a GitHub repository
/// are left out.
pub(crate) fn read(body: &Value, page: Option<u32>) -> Vec<Scroll> {
    let first = page.map(|page| page * PER_PAGE + 1);
    body["skills"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(at, s)| {
            let source = s["source"].as_str()?.to_string();
            let skill = s["skillId"].as_str()?.to_string();
            if !safe_source(&source) || !safe_name(&skill) {
                return None;
            }
            Some(Scroll {
                name: s["name"]
                    .as_str()
                    .unwrap_or(&skill)
                    .to_string(),
                source,
                skill,
                installs: s["installs"].as_u64().unwrap_or_default(),
                rank: first.map(|first| first + at as u32),
                official: s["isOfficial"]
                    .as_bool()
                    .unwrap_or_default(),
            })
        })
        .collect()
}

/// True when skills.sh says there are pages past the one in `body`.
pub(crate) fn has_more(body: &Value) -> bool {
    body["hasMore"] == Value::Bool(true)
}

/// True when `scroll` names a skill of a GitHub repository, so its
/// files may be asked for.
pub(crate) fn askable(scroll: &Scroll) -> bool {
    safe_source(&scroll.source) && safe_name(&scroll.skill)
}

/// The files of one skill, as skills.sh keeps them.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub(crate) struct Unrolled {
    /// Each file's path in the skill's folder and its text, SKILL.md
    /// first.
    pub files: Vec<(String, String)>,
    /// The files left out, each with the reason.
    pub left: Vec<String>,
}

impl Unrolled {
    /// The text of its SKILL.md.
    pub fn text(&self) -> &str {
        self.files.first().map_or("", |(_, text)| text)
    }

    /// What its SKILL.md says it does.
    pub fn description(&self) -> String {
        front(self.text(), "description").unwrap_or_default()
    }

    /// What its SKILL.md says past its front matter.
    pub fn body(&self) -> &str {
        let text = self.text();
        match front(text, "name") {
            Some(_) => text
                .strip_prefix("---")
                .and_then(|rest| rest.split_once("\n---"))
                .map_or(text, |(_, body)| body),
            None => text,
        }
        .trim()
    }
}

/// The files in skills.sh's answer for one skill. A path that leads
/// out of its folder fails it whole; a file too large, or past the
/// most kept, is left out.
pub(crate) fn read_files(body: &Value) -> Result<Unrolled, String> {
    let mut unrolled = Unrolled::default();
    let mut size = 0;
    for file in body["files"].as_array().into_iter().flatten() {
        let Some(path) = file["path"].as_str() else {
            continue;
        };
        if !safe_path(path) {
            return Err(format!(
                "skills.sh gave a file outside the skill: {path}"
            ));
        }
        let skill_md = path.eq_ignore_ascii_case("SKILL.md");
        let path = if skill_md { "SKILL.md" } else { path };
        if unrolled.files.iter().any(|(had, _)| had == path) {
            continue;
        }
        let Some(text) = file["contents"].as_str() else {
            unrolled.left.push(format!("{path} (not text)"));
            continue;
        };
        if text.len() > FILE_MOST
            || size + text.len() > SKILL_MOST
            || unrolled.files.len() >= FILES_MOST
        {
            if skill_md {
                return Err(
                    "its SKILL.md is too large to keep".into()
                );
            }
            unrolled.left.push(format!("{path} (too large)"));
            continue;
        }
        size += text.len();
        let file = (path.to_string(), text.to_string());
        match skill_md {
            true => unrolled.files.insert(0, file),
            false => unrolled.files.push(file),
        }
    }
    if unrolled
        .files
        .first()
        .is_none_or(|(path, _)| path != "SKILL.md")
    {
        return Err("skills.sh has no SKILL.md for it".into());
    }
    Ok(unrolled)
}

/// The field `key` of the front matter `text` opens with: on one
/// line, quoted or not, or folded over the indented lines after a
/// `>` or `|`.
pub(crate) fn front(text: &str, key: &str) -> Option<String> {
    let front = text.strip_prefix("---")?.split("\n---").next()?;
    let mut lines = front.lines();
    let value = lines.find_map(|line| {
        let (k, v) = line.split_once(':')?;
        (k.trim() == key).then(|| v.trim())
    })?;
    if value.starts_with(['>', '|']) {
        let folded = lines
            .take_while(|line| {
                line.starts_with([' ', '\t'])
                    || line.trim().is_empty()
            })
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>();
        return Some(folded.join(" "));
    }
    let unquoted = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|v| v.strip_suffix('\''))
        })
        .unwrap_or(value);
    Some(unquoted.to_string())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_leaderboard_is_read_with_ranks_and_only_github_skills() {
        let body = json!({"skills": [
            {"source": "vercel-labs/skills", "skillId": "find-skills",
             "name": "find-skills", "installs": 3881956,
             "isOfficial": true},
            {"source": "open.feishu.cn", "skillId": "lark-doc",
             "name": "lark-doc", "installs": 900000},
            {"source": "someone/repo", "skillId": "../escape",
             "name": "escape", "installs": 5},
            {"source": "anthropics/skills", "skillId": "frontend-design",
             "name": "Frontend Design", "installs": 800000}
        ], "hasMore": true});
        let scrolls = read(&body, Some(1));
        let paths =
            scrolls.iter().map(Scroll::path).collect::<Vec<_>>();
        assert_eq!(
            paths,
            [
                "vercel-labs/skills/find-skills",
                "anthropics/skills/frontend-design"
            ]
        );
        assert!(scrolls[0].official);
        // A rank is the place on the board, even past those left out.
        assert_eq!(scrolls[1].rank, Some(204));
        assert!(has_more(&body));
        assert_eq!(read(&body, None)[0].rank, None);
    }

    #[test]
    fn a_skill_s_files_stay_in_its_folder() {
        let body = json!({"files": [
            {"path": "agents/openai.yaml", "contents": "interface:\n"},
            {"path": "skill.md", "contents": "---\nname: tdd\n---\n"},
            {"path": "big.bin", "contents": "x".repeat(FILE_MOST + 1)},
            {"path": "logo.png", "contents": null}
        ]});
        let unrolled = read_files(&body).unwrap();
        assert_eq!(unrolled.files[0].0, "SKILL.md");
        assert_eq!(unrolled.files[1].0, "agents/openai.yaml");
        assert_eq!(
            unrolled.left,
            ["big.bin (too large)", "logo.png (not text)"]
        );
        for bad in [
            "../x",
            "a/../../x",
            "/etc/passwd",
            "a\\..\\x",
            "C:x",
            "a//b",
            "./x",
        ] {
            let body = json!({"files": [
                {"path": "SKILL.md", "contents": "---\nname: a\n---\n"},
                {"path": bad, "contents": "x"}
            ]});
            assert!(read_files(&body).is_err(), "{bad}");
        }
        let none = json!({"files": [{"path": "README.md", "contents": "x"}]});
        assert!(read_files(&none).is_err());
        assert!(
            !safe_name("..")
                && !safe_name(".hidden")
                && !safe_name("a/b")
        );
    }

    #[test]
    fn a_description_is_read_however_it_is_written() {
        let plain =
            "---\nname: tdd\ndescription: Test first.\n---\nBody";
        assert_eq!(
            front(plain, "description").unwrap(),
            "Test first."
        );
        let quoted = "---\nname: x\ndescription: \"Says: hi\"\n---\n";
        assert_eq!(front(quoted, "description").unwrap(), "Says: hi");
        let folded = "---\nname: x\ndescription: >-\n  Two lines\n  \
                      of words.\nlicense: MIT\n---\n";
        assert_eq!(
            front(folded, "description").unwrap(),
            "Two lines of words."
        );
        assert_eq!(front("# No front matter", "name"), None);
    }
}
