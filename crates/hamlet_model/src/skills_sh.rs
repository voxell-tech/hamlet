//! The skills listed on skills.sh, kept in `<home>/skills_sh.json`,
//! and fetching one to keep. skills.sh lists the skills kept in
//! GitHub repositories, by how often they were installed.

use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The address skills.sh answers at.
const SITE: &str = "https://skills.sh";

/// The name Hamlet gives itself to skills.sh.
const AGENT: &str = concat!("hamlet/", env!("CARGO_PKG_VERSION"));

/// The time the listing stays fresh before skills.sh is asked again,
/// in milliseconds.
pub const FRESH: u64 = 24 * 60 * 60 * 1000;

/// The wait for skills.sh to answer.
const ASK_TIMEOUT: Duration = Duration::from_secs(20);

/// The pages of the leaderboard asked for, of 200 skills each.
const PAGES: u32 = 3;

/// The most skills a search asks for.
const FOUND_MOST: usize = 50;

/// The most bytes of an answer read.
const ANSWER_MOST: usize = 8 * 1024 * 1024;

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
#[derive(
    Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default,
)]
#[serde(default)]
pub struct Scroll {
    /// The GitHub repository it is kept in, as `owner/repo`.
    pub source: String,
    /// skills.sh's name for it in that repository, and its folder's
    /// name once shelved.
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
    pub fn id(&self) -> String {
        format!("{}/{}", self.source, self.skill)
    }

    /// Its page on skills.sh.
    pub fn page(&self) -> String {
        format!("{SITE}/{}", self.id())
    }

    /// The repository it is kept in, on GitHub.
    pub fn repository(&self) -> String {
        format!("https://github.com/{}", self.source)
    }
}

/// The listing as its file keeps it.
#[derive(
    Serialize, Deserialize, Clone, Debug, PartialEq, Default,
)]
#[serde(default)]
pub struct Scrolls {
    /// The time skills.sh last answered, in milliseconds since 1970.
    pub fetched: u64,
    /// The leaderboard's skills, most installed first.
    pub scrolls: Vec<Scroll>,
}

/// The path of the listing of `home`.
pub fn path(home: &Path) -> PathBuf {
    home.join("skills_sh.json")
}

impl Scrolls {
    /// The listing of `home`, empty when the file is missing or
    /// cannot be read.
    pub fn load(home: &Path) -> Scrolls {
        std::fs::read_to_string(path(home))
            .ok()
            .and_then(|text| {
                serde_json::from_str::<Scrolls>(&text).ok()
            })
            .unwrap_or_default()
    }

    pub fn save(&self, home: &Path) -> Result<()> {
        let text = serde_json::to_string(self)?;
        std::fs::write(path(home), text).with_context(|| {
            format!("writing {}", path(home).display())
        })
    }

    /// True when skills.sh should be asked again at `now`, in
    /// milliseconds since 1970.
    pub fn stale(&self, now: u64) -> bool {
        now.saturating_sub(self.fetched) >= FRESH
    }

    /// The skill of skills.sh's id `id`.
    pub fn get(&self, id: &str) -> Option<&Scroll> {
        self.scrolls.iter().find(|scroll| scroll.id() == id)
    }
}

/// True when `name` can be a folder's name: letters, digits, `.`,
/// `_` and `-`, beginning with a letter or a digit.
pub fn safe_name(name: &str) -> bool {
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
pub fn safe_path(path: &str) -> bool {
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

/// The skills in skills.sh's answer of a page of its leaderboard or a
/// search, ranked from `first` where it is some. Those kept anywhere
/// but a GitHub repository are left out.
pub(crate) fn read(body: &Value, first: Option<u32>) -> Vec<Scroll> {
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

/// Asks skills.sh for the most installed skills of all time.
pub async fn fetch() -> Result<Vec<Scroll>> {
    let mut scrolls = Vec::new();
    for page in 0..PAGES {
        let url = format!("{SITE}/api/skills/all-time/{page}");
        let body = ask(&url).await?;
        let first = page * 200 + 1;
        scrolls.extend(read(&body, Some(first)));
        if body["hasMore"] != Value::Bool(true) {
            break;
        }
    }
    Ok(scrolls)
}

/// Asks skills.sh for the skills that `words` name or describe.
pub async fn search(words: &str) -> Result<Vec<Scroll>> {
    let url = reqwest::Url::parse_with_params(
        &format!("{SITE}/api/search"),
        &[("q", words.trim()), ("limit", &FOUND_MOST.to_string())],
    )?;
    Ok(read(&ask(url.as_str()).await?, None))
}

async fn ask(url: &str) -> Result<Value> {
    let client = reqwest::Client::builder()
        .timeout(ASK_TIMEOUT)
        .user_agent(AGENT)
        .build()?;
    let mut resp = client
        .get(url)
        .send()
        .await
        .context("could not reach skills.sh")?;
    let status = resp.status();
    let mut bytes = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        bytes.extend_from_slice(&chunk);
        anyhow::ensure!(
            bytes.len() <= ANSWER_MOST,
            "skills.sh answered with more than {} MB",
            ANSWER_MOST / 1024 / 1024
        );
    }
    let body =
        serde_json::from_slice::<Value>(&bytes).unwrap_or_default();
    if status == reqwest::StatusCode::NOT_FOUND {
        bail!("skills.sh does not have it");
    }
    anyhow::ensure!(
        status.is_success(),
        "HTTP {status}: {}",
        body["message"].as_str().unwrap_or_default()
    );
    Ok(body)
}

/// The files of one skill, as skills.sh keeps them.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Unrolled {
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

    /// The bytes of all its files.
    pub fn size(&self) -> usize {
        self.files.iter().map(|(_, text)| text.len()).sum()
    }
}

/// The files in skills.sh's answer for one skill. A path that leads
/// out of its folder fails it whole; a file too large, or past the
/// most kept, is left out.
pub(crate) fn read_files(body: &Value) -> Result<Unrolled> {
    let mut unrolled = Unrolled::default();
    let mut size = 0;
    for file in body["files"].as_array().into_iter().flatten() {
        let Some(path) = file["path"].as_str() else {
            continue;
        };
        if !safe_path(path) {
            bail!("skills.sh gave a file outside the skill: {path}");
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
                bail!("its SKILL.md is too large to keep");
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
        bail!("skills.sh has no SKILL.md for it");
    }
    Ok(unrolled)
}

/// Asks skills.sh for the files of `scroll`.
pub async fn unroll(scroll: &Scroll) -> Result<Unrolled> {
    anyhow::ensure!(
        safe_source(&scroll.source) && safe_name(&scroll.skill),
        "`{}` is no skill of a GitHub repository",
        scroll.id()
    );
    let url = format!("{SITE}/api/download/{}", scroll.id());
    read_files(&ask(&url).await?)
}

/// The field `key` of the front matter `text` opens with: on one
/// line, quoted or not, or folded over the indented lines after a
/// `>` or `|`.
pub fn front(text: &str, key: &str) -> Option<String> {
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

/// The order skills are shown in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Order {
    #[default]
    Installs,
    Name,
}

impl Order {
    pub const ALL: [Order; 2] = [Order::Installs, Order::Name];

    pub fn name(self) -> &'static str {
        match self {
            Order::Installs => "Most installed",
            Order::Name => "Name",
        }
    }
}

/// The `scrolls` whose name, repository or folder holds each of
/// `words`, in `order`.
pub fn shown<'a>(
    scrolls: &'a [Scroll],
    words: &str,
    order: Order,
) -> Vec<&'a Scroll> {
    let words = words.to_lowercase();
    let mut shown = scrolls
        .iter()
        .filter(|scroll| {
            let named = format!(
                "{} {} {}",
                scroll.name, scroll.source, scroll.skill
            )
            .to_lowercase();
            words.split_whitespace().all(|word| named.contains(word))
        })
        .collect::<Vec<_>>();
    let by_name = |a: &Scroll, b: &Scroll| {
        a.name.to_lowercase().cmp(&b.name.to_lowercase())
    };
    match order {
        Order::Installs => shown.sort_by(|a, b| {
            b.installs.cmp(&a.installs).then_with(|| by_name(a, b))
        }),
        Order::Name => shown.sort_by(|a, b| {
            by_name(a, b).then_with(|| a.source.cmp(&b.source))
        }),
    }
    shown
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// A page of skills.sh's leaderboard, cut down from a real one.
    fn board() -> Value {
        json!({"skills": [
            {"source": "vercel-labs/skills", "skillId": "find-skills",
             "name": "find-skills", "installs": 3881956,
             "weeklyInstalls": [99722, 102581], "isOfficial": true},
            {"source": "mattpocock/skills", "skillId": "tdd",
             "name": "tdd", "installs": 1094811,
             "weeklyInstalls": [56603, 51060]},
            {"source": "open.feishu.cn", "skillId": "lark-doc",
             "name": "lark-doc", "installs": 900000},
            {"source": "someone/repo", "skillId": "../escape",
             "name": "escape", "installs": 5},
            {"source": "anthropics/skills", "skillId": "frontend-design",
             "name": "Frontend Design", "installs": 800000}
        ], "total": 9908, "hasMore": true, "page": 1})
    }

    #[test]
    fn the_leaderboard_is_read_with_ranks_and_only_github_skills() {
        let scrolls = read(&board(), Some(201));
        let ids = scrolls.iter().map(Scroll::id).collect::<Vec<_>>();
        assert_eq!(
            ids,
            [
                "vercel-labs/skills/find-skills",
                "mattpocock/skills/tdd",
                "anthropics/skills/frontend-design"
            ]
        );
        assert_eq!(scrolls[0].rank, Some(201));
        assert!(scrolls[0].official && !scrolls[1].official);
        // A rank is the place on the board, even past those left out.
        assert_eq!(scrolls[2].rank, Some(205));
        assert_eq!(scrolls[2].name, "Frontend Design");
        assert_eq!(
            scrolls[1].page(),
            "https://skills.sh/mattpocock/skills/tdd"
        );
        // A search is the same shape, with no ranks.
        let found = json!({"query": "tdd", "skills": [
            {"id": "mattpocock/skills/tdd", "source": "mattpocock/skills",
             "skillId": "tdd", "name": "tdd", "installs": 1094811}
        ]});
        assert_eq!(read(&found, None)[0].rank, None);
    }

    #[test]
    fn a_skill_s_files_stay_in_its_folder() {
        let body = json!({"files": [
            {"path": "agents/openai.yaml", "contents": "interface:\n"},
            {"path": "skill.md", "contents": "---\nname: tdd\n---\n"},
            {"path": "big.bin", "contents": "x".repeat(FILE_MOST + 1)},
            {"path": "logo.png", "contents": null}
        ], "hash": "b86d"});
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
            assert!(!safe_path(bad), "{bad}");
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

    #[test]
    fn the_listing_comes_back_from_its_file_and_is_searched() {
        let home = hamlet_testing::Scratch::new("skills-sh-file");
        let scrolls = Scrolls {
            fetched: 5,
            scrolls: read(&board(), Some(1)),
        };
        scrolls.save(&home).unwrap();
        assert_eq!(Scrolls::load(&home), scrolls);
        assert!(
            scrolls.stale(5 + FRESH) && !scrolls.stale(4 + FRESH)
        );

        let names = |words: &str, order| {
            shown(&scrolls.scrolls, words, order)
                .iter()
                .map(|scroll| scroll.skill.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names("", Order::Name),
            ["find-skills", "frontend-design", "tdd"]
        );
        assert_eq!(
            names("SKILLS f", Order::Installs),
            ["find-skills", "frontend-design"]
        );
        assert_eq!(names("mattpocock", Order::Installs), ["tdd"]);
    }

    #[tokio::test]
    #[cfg(feature = "live-tests")]
    async fn live_skills_sh_lists_and_unrolls() {
        let scrolls = fetch().await.unwrap();
        assert!(scrolls.len() > 100, "{}", scrolls.len());
        let found = search("tdd").await.unwrap();
        assert!(!found.is_empty());
        let unrolled = unroll(&scrolls[0]).await.unwrap();
        assert!(front(unrolled.text(), "name").is_some());
    }
}
