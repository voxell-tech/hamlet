//! Built-in tools (a shell, `edit`, and the reading tools of a
//! hamlet that works on a folder) and the tools added by hand.

pub mod change;
mod custom;
mod edit;
mod reading;
mod shell;

use std::path::{Path, PathBuf};

use hamlet_home::{Home, Project};
use hamlet_rules::library::Library;
use hamlet_sandbox::{self, Policy, Sandbox};
use serde_json::{Value, json};

pub use crate::tools::change::Change;
use crate::tools::custom::{CustomTool, reserved};
use crate::tools::edit::edit;
use crate::tools::reading::Sight;
use crate::tools::shell::Shell;

/// A tool call as the model made it.
pub(crate) struct Call {
    pub id: String,
    pub name: String,
    pub args: String,
}

/// The reach of a call, as far as the tools know.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Reach {
    /// `net` when the caller said the command needs the network.
    Shell { command: String, net: bool },
    /// The in-process edit of a file.
    Edit,
    /// An in-process read, which changes nothing.
    Read,
    /// `net` when its manifest asks for the network.
    CustomTool { net: bool },
    /// None of these tools.
    Other,
}

/// The reach one call is given past its sandbox.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Leave {
    pub net: bool,
    /// The chief allowed an act the limits reserve to the chief.
    pub reserved: bool,
}

/// A tool call's result, as the agent's log keeps it.
pub(crate) struct Outcome {
    pub output: String,
    pub exit_code: i32,
    pub policy: String,
    pub degraded: bool,
}

impl Outcome {
    pub(crate) fn blocked(reason: &str) -> Self {
        Outcome::plain(1, format!("Error: blocked: {reason}"))
    }

    pub(crate) fn plain(
        exit_code: i32,
        output: impl Into<String>,
    ) -> Self {
        Outcome {
            output: output.into(),
            exit_code,
            policy: String::new(),
            degraded: false,
        }
    }
}

/// The tools of one agent, in its house.
pub struct Tools {
    home: Home,
    /// The skills every agent shares, which a command may read.
    library: Option<Library>,
    sandbox: &'static dyn Sandbox,
    shell: Shell,
    custom_tools: Vec<CustomTool>,
    site: Option<Site>,
    /// The workspaces of other agents that a command may read.
    led: Vec<PathBuf>,
}

/// The folder the agent's hamlet works on.
struct Site {
    project: Project,
    /// Hamlet's own folder, which stays out of reach from there.
    hamlet: PathBuf,
}

impl Tools {
    /// The built-in tools for the agent that lives in `home`.
    pub fn new(home: Home, sandbox: &'static dyn Sandbox) -> Self {
        Tools {
            home,
            library: None,
            sandbox,
            shell: Shell::detect(),
            custom_tools: vec![],
            site: None,
            led: vec![],
        }
    }

    /// Lets a command read the workspaces `led`, each with its
    /// agent's name.
    pub(crate) fn lead(&mut self, led: Vec<(String, PathBuf)>) {
        self.led =
            led.into_iter().map(|(_, workspace)| workspace).collect();
    }

    /// Points the tools at the folder the agent's hamlet works on,
    /// or at none. `hamlet` is Hamlet's own folder.
    pub(crate) fn work_on(
        &mut self,
        project: Option<Project>,
        hamlet: &Path,
    ) {
        self.site = project.map(|project| Site {
            project,
            hamlet: hamlet
                .canonicalize()
                .unwrap_or_else(|_| hamlet.to_path_buf()),
        });
    }

    pub(crate) fn project(&self) -> Option<&Project> {
        self.site.as_ref().map(|site| &site.project)
    }

    /// The folders a command may read without writing: the tools
    /// added by hand, the library and the workspaces of the
    /// agents it leads.
    fn shared(&self) -> Vec<PathBuf> {
        std::iter::once(self.home.tools())
            .chain(
                self.library
                    .iter()
                    .map(|library| library.root().to_path_buf()),
            )
            .chain(self.led.iter().cloned())
            .collect()
    }

    /// These tools, with the library of the home to read from.
    pub(crate) fn with_library(mut self, library: Library) -> Self {
        self.library = Some(library);
        self
    }

    pub(crate) fn library(&self) -> Option<&Library> {
        self.library.as_ref()
    }

    pub(crate) fn home(&self) -> &Home {
        &self.home
    }

    /// The same tools, working in another agent's house.
    pub(crate) fn in_house(&self, home: Home) -> Self {
        Tools {
            library: self.library.clone(),
            ..Tools::new(home, self.sandbox)
        }
    }

    /// Re-reads `tools/`. Returns a line per manifest that failed
    /// to load.
    pub(crate) fn rescan(&mut self) -> Vec<String> {
        self.custom_tools.clear();
        let mut errors = vec![];
        let Ok(entries) = std::fs::read_dir(self.home.tools()) else {
            return errors;
        };
        let mut dirs = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect::<Vec<_>>();
        dirs.sort();
        for dir in dirs {
            match CustomTool::load(&dir, self.sandbox.base_env()) {
                Ok(p) if reserved(&p.name) => errors.push(format!(
                    "{}: name '{}' is reserved",
                    dir.display(),
                    p.name
                )),
                Ok(p)
                    if self
                        .custom_tools
                        .iter()
                        .any(|q| q.name == p.name) =>
                {
                    errors.push(format!(
                        "{}: name '{}' is taken by another tool",
                        dir.display(),
                        p.name
                    ))
                }
                Ok(p) => self.custom_tools.push(p),
                Err(e) => {
                    errors.push(format!("{}: {e}", dir.display()))
                }
            }
        }
        errors
    }

    /// Re-reads `tools/`. Returns the words of the system prompt
    /// on those that failed to load.
    pub(crate) fn faults(&mut self) -> String {
        let errors = self.rescan();
        match errors.is_empty() {
            true => String::new(),
            false => format!(
                "\nTools that failed to load:\n{}",
                errors.join("\n")
            ),
        }
    }

    pub(crate) fn definitions(&self) -> Vec<Value> {
        let (runs, edits, path) = match &self.site {
            Some(_) => (
                self.shell.description.replace(
                    "in the workspace",
                    "in the project folder",
                ),
                "a file of the project or of your workspace",
                "Relative to the project, or a full path.",
            ),
            None => (
                self.shell.description.to_string(),
                "a workspace file",
                "Relative to the workspace.",
            ),
        };
        let mut defs = vec![
            json!({"type": "function", "function": {
                "name": self.shell.name,
                "description": runs,
                "parameters": {"type": "object", "properties": {
                    "command": {"type": "string"},
                    "network": {"type": "boolean", "description": "True when the command needs the network."}},
                    "required": ["command"]}}}),
            json!({"type": "function", "function": {
                "name": "edit",
                "description": format!("Replace one exact occurrence of `old` with `new` in {edits}. \
                                With an empty `old` and a missing file, creates the file with `new`."),
                "parameters": {"type": "object", "properties": {
                    "path": {"type": "string", "description": path},
                    "old": {"type": "string"}, "new": {"type": "string"}},
                    "required": ["path", "old", "new"]}}}),
        ];
        if self.site.is_some() {
            defs.extend(reading::definitions());
        }
        defs.extend(
            self.custom_tools.iter().map(|p| p.definition.clone()),
        );
        defs
    }

    pub(crate) fn reach(&self, call: &Call) -> Reach {
        let args = serde_json::from_str::<Value>(&call.args)
            .unwrap_or_default();
        match call.name.as_str() {
            n if n == self.shell.name => Reach::Shell {
                command: args["command"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                net: args["network"].as_bool().unwrap_or(false),
            },
            "edit" => Reach::Edit,
            n if self.site.is_some()
                && reading::NAMES.contains(&n) =>
            {
                Reach::Read
            }
            n => match self.custom_tools.iter().find(|p| p.name == n)
            {
                Some(p) => Reach::CustomTool { net: p.net },
                None => Reach::Other,
            },
        }
    }

    pub(crate) async fn call(
        &self,
        call: &Call,
        leave: Leave,
    ) -> Outcome {
        match serde_json::from_str::<Value>(if call.args.is_empty() {
            "{}"
        } else {
            &call.args
        }) {
            Err(e) => Outcome::plain(
                1,
                format!("Error: arguments are not valid JSON: {e}"),
            ),
            Ok(args) => self.dispatch(&call.name, args, leave).await,
        }
    }

    async fn dispatch(
        &self,
        name: &str,
        args: Value,
        leave: Leave,
    ) -> Outcome {
        match name {
            n if n == self.shell.name => match args["command"]
                .as_str()
            {
                Some(cmd) => {
                    let argv = self.shell.argv(cmd);
                    let cwd = match &self.site {
                        Some(site) => &site.project.folder,
                        None => &self.home.workspace,
                    };
                    if !cwd.is_dir() {
                        return Outcome::plain(
                            1,
                            format!(
                                "Error: the folder {} is gone. Tell \
                                 the chief.",
                                cwd.display()
                            ),
                        );
                    }
                    self.run(
                        &self.shell_policy(leave),
                        &argv,
                        cwd,
                        None,
                    )
                    .await
                }
                None => Outcome::plain(1, "Error: missing `command`"),
            },
            "edit" => edit(
                &self.home.workspace,
                self.project().map(|project| &*project.folder),
                &args,
            ),
            n if self.site.is_some()
                && reading::NAMES.contains(&n) =>
            {
                self.look(n, &args)
            }
            _ => match self
                .custom_tools
                .iter()
                .find(|p| p.name == name)
            {
                Some(p) => {
                    let mut argv = p.exec.clone();
                    if argv[0].starts_with("./") {
                        argv[0] = p
                            .dir
                            .join(&argv[0][2..])
                            .to_string_lossy()
                            .into_owned();
                    }
                    let stdin = args.to_string();
                    // Domain allowlists need the Phase 2 proxy;
                    // until then network is all or nothing.
                    let policy = Policy {
                        net: p.net && leave.net,
                        ..p.policy.clone()
                    };
                    self.run(
                        &policy,
                        &argv,
                        &p.dir,
                        Some(stdin.as_bytes()),
                    )
                    .await
                }
                None => Outcome::plain(1, self.unknown(name)),
            },
        }
    }

    /// The answer to a call of no tool. A skill called as one is
    /// told how skills are run, with its instructions.
    fn unknown(&self, name: &str) -> String {
        // Its own, or one of the library's.
        let shared = self.library.iter().flat_map(Library::index);
        let skill =
            self.home.skill_index().into_iter().chain(shared).find(
                |skill| {
                    skill.name == name
                        || skill.name == name.replace('_', "-")
                },
            );
        let Some(skill) = skill else {
            return format!("Error: unknown tool `{name}`");
        };
        let text =
            std::fs::read_to_string(&skill.path).unwrap_or_default();
        format!(
            "Error: `{name}` is a skill, not a tool. A skill is run \
             with the `{}` tool, as its instructions say:\n\n{text}",
            self.shell.name
        )
    }

    /// Runs one of the reading tools.
    fn look(&self, name: &str, args: &Value) -> Outcome {
        let Some(site) = &self.site else {
            return Outcome::plain(1, self.unknown(name));
        };
        let mut shown = self.shared();
        shown.push(self.home.workspace.clone());
        let shown = shown
            .into_iter()
            .filter_map(|path| path.canonicalize().ok())
            .collect::<Vec<_>>();
        let sight = Sight {
            base: &site.project.folder,
            hidden: &site.hamlet,
            shown: &shown,
        };
        let done = match name {
            "read" => reading::read(&sight, args),
            "search" => reading::search(&sight, args),
            _ => reading::find(&sight, args),
        };
        match done {
            Ok(text) => Outcome::plain(0, text),
            Err(e) => Outcome::plain(1, format!("Error: {e}")),
        }
    }

    fn shell_policy(&self, leave: Leave) -> Policy {
        let policy = Policy {
            // Read-only so bash can't rewrite code that later runs
            // with a broader policy.
            ro: self.shared(),
            rw: vec![self.home.workspace.clone()],
            net: leave.net,
            env: self.sandbox.base_env(),
            ..Policy::default()
        };
        let Some(site) = &self.site else {
            return policy;
        };
        let folder = &site.project.folder;
        let git = folder.join(".git");
        Policy {
            rw: vec![folder.clone(), self.home.workspace.clone()],
            open: true,
            hidden: vec![site.hamlet.clone()],
            // A hook or a setting written here is run later with no
            // sandbox. Where there is no repository none can be
            // made, since it would come with both.
            locked: if git.is_dir() {
                vec![git.join("hooks"), git.join("config")]
            } else {
                vec![git]
            },
            // The chief's own, so the tools of the project are found.
            env: hamlet_sandbox::host_env(),
            ..policy
        }
    }

    async fn run(
        &self,
        policy: &Policy,
        argv: &[String],
        cwd: &Path,
        stdin: Option<&[u8]>,
    ) -> Outcome {
        let hash = policy.hash(self.sandbox.name());
        let degraded = self.sandbox.degraded();
        match hamlet_sandbox::run(
            self.sandbox,
            policy,
            argv,
            cwd,
            stdin,
        )
        .await
        {
            Ok(r) => {
                let mut output = r.output;
                if r.exit_code != 0
                    && !degraded
                    && output.contains("Operation not permitted")
                {
                    output.push_str(match &self.site {
                        Some(_) => OUTSIDE_PROJECT,
                        None => OUTSIDE,
                    });
                }
                if r.timed_out {
                    output.push_str(&format!(
                        "\n[timed out after {}s]",
                        policy.limits.wall_secs
                    ));
                }
                Outcome {
                    output,
                    exit_code: r.exit_code,
                    policy: hash,
                    degraded,
                }
            }
            Err(e) => Outcome {
                output: format!("Error: {e}"),
                exit_code: -1,
                policy: hash,
                degraded,
            },
        }
    }
}

/// Said after a command the sandbox stopped.
const OUTSIDE: &str = "\n[The sandbox lets a command write only \
                       inside the workspace, which is its working \
                       directory. Use a relative path there, not \
                       /tmp or another folder.]";

const OUTSIDE_PROJECT: &str =
    "\n[The sandbox lets a command write only in the project folder \
     and in your own workspace, never in .git/hooks or .git/config, \
     and keeps Hamlet's own folder closed to it.]";

#[cfg(test)]
mod tests {
    use hamlet_sandbox::Unsandboxed;
    use hamlet_testing::Scratch;

    use super::*;

    fn setup(tag: &str) -> (Scratch, Tools) {
        let dir = Scratch::new(&format!("tools-{tag}"));
        let home = Home::open(dir.to_path_buf(), None).unwrap();
        let tools = Tools::new(home, &Unsandboxed)
            .with_library(Library::open(&dir).unwrap());
        (dir, tools)
    }

    fn call(name: &str, args: Value) -> Call {
        Call {
            id: "c1".into(),
            name: name.into(),
            args: args.to_string(),
        }
    }

    #[test]
    fn a_project_opens_reading_and_keeps_hamlets_folder_shut() {
        let (dir, mut t) = setup("policy-project");
        let project = Scratch::new("tools-policy-folder");
        let plain = t.shell_policy(Leave::default());
        assert!(!plain.open && plain.locked.is_empty());
        assert_eq!(t.definitions().len(), 2);
        t.work_on(
            Some(Project {
                folder: project.to_path_buf(),
                manual: true,
            }),
            &dir,
        );
        let policy = t.shell_policy(Leave::default());
        assert!(policy.open);
        assert_eq!(policy.hidden, [dir.to_path_buf()]);
        assert_eq!(
            policy.rw,
            [project.to_path_buf(), t.home.workspace.clone()]
        );
        // No repository, so none may be made.
        assert_eq!(policy.locked, [project.join(".git")]);
        std::fs::create_dir(project.join(".git")).unwrap();
        assert_eq!(
            t.shell_policy(Leave::default()).locked,
            [project.join(".git/hooks"), project.join(".git/config")]
        );
        // The reading tools come with the project, and never wait.
        assert_eq!(t.definitions().len(), 5);
        let read = call("read", json!({"path": "x"}));
        assert_eq!(t.reach(&read), Reach::Read);
    }

    #[tokio::test]
    async fn an_agent_cannot_write_its_character() {
        let (dir, mut t) = setup("character");
        let project = Scratch::new("tools-character-folder");
        let character = t.home.character();
        std::fs::write(&character, "Calm.\n").unwrap();
        for site in [false, true] {
            if site {
                t.work_on(
                    Some(Project {
                        folder: project.to_path_buf(),
                        manual: false,
                    }),
                    &dir,
                );
            }
            for path in
                ["../CHARACTER.md", character.to_str().unwrap()]
            {
                for old in ["", "Calm."] {
                    let args =
                        json!({"path": path, "old": old, "new": "x"});
                    let o = t
                        .call(&call("edit", args), Leave::default())
                        .await;
                    assert_eq!(o.exit_code, 1, "{}", o.output);
                }
            }
            // No folder a command may write in holds it.
            let policy = t.shell_policy(Leave::default());
            assert!(
                policy
                    .rw
                    .iter()
                    .all(|open| !character.starts_with(open))
            );
        }
        assert_eq!(
            std::fs::read_to_string(&character).unwrap(),
            "Calm.\n"
        );
        assert!(!project.join("CHARACTER.md").exists());
    }

    #[tokio::test]
    async fn bash_runs_in_workspace() {
        let (_dir, t) = setup("bash");
        let o = t
            .call(
                &call(
                    "bash",
                    json!({"command": "pwd; echo err >&2; exit 3"}),
                ),
                Leave::default(),
            )
            .await;
        assert_eq!(o.exit_code, 3);
        assert!(o.output.contains("hamlet-tools-bash"));
        assert!(o.output.contains("err"));
        assert!(o.degraded);
        // A skill called as a tool is told how skills are run.
        let skill = t
            .call(
                &call("web_search", json!({"query": "x"})),
                Leave::default(),
            )
            .await;
        assert!(skill.output.contains("is a skill, not a tool"));
        assert!(skill.output.contains("scripts/search.py"));
        // A command may read the library, and write only at home.
        let policy = t.shell_policy(Leave::default());
        let library = t.library().unwrap().root().to_path_buf();
        assert!(policy.ro.contains(&library));
        assert!(!policy.rw.contains(&library));
        assert_eq!(
            t.call(
                &Call {
                    id: "x".into(),
                    name: "bash".into(),
                    args: "{bad".into()
                },
                Leave::default()
            )
            .await
            .exit_code,
            1
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn custom_tools() {
        let (_dir, mut t) = setup("custom");
        let dir = t.home.tools().join("shout");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("manifest.json"),
            r#"{"name":"shout","description":"Uppercase text","exec":["./run.sh"],
                "parameters":{"type":"object","properties":{"text":{"type":"string"}}}}"#,
        )
        .unwrap();
        std::fs::write(dir.join("run.sh"), "#!/bin/sh\ntr a-z A-Z\n")
            .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            dir.join("run.sh"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        let bad = t.home.tools().join("bash");
        std::fs::create_dir_all(&bad).unwrap();
        std::fs::write(
            bad.join("manifest.json"),
            r#"{"name":"bash","exec":["x"]}"#,
        )
        .unwrap();

        let twin = t.home.tools().join("shout-again");
        std::fs::create_dir_all(&twin).unwrap();
        std::fs::write(
            twin.join("manifest.json"),
            r#"{"name":"shout","exec":["x"]}"#,
        )
        .unwrap();

        let errors = t.rescan();
        assert_eq!(errors.len(), 2, "{errors:?}");
        assert!(errors[0].contains("reserved"), "{errors:?}");
        assert!(errors[1].contains("taken"), "{errors:?}");
        assert_eq!(t.definitions().len(), 3);
        let shout = call("shout", json!({"text": "hi"}));
        assert_eq!(t.reach(&shout), Reach::CustomTool { net: false });
        assert_eq!(
            t.reach(&call(
                "bash",
                json!({"command": "ls", "network": true})
            )),
            Reach::Shell {
                command: "ls".into(),
                net: true
            }
        );
        let o = t.call(&shout, Leave::default()).await;
        assert!(
            o.output.contains(r#"{"TEXT":"HI"}"#),
            "{}",
            o.output
        );

        assert_eq!(
            t.call(&call("nope", json!({})), Leave::default())
                .await
                .exit_code,
            1
        );
    }
}
