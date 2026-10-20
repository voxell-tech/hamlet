//! The shell an agent's command tool runs.

use std::path::PathBuf;

/// The shell the agent's command tool runs: bash where available,
/// else PowerShell on Windows.
pub(super) struct Shell {
    pub(super) name: &'static str,
    pub(super) description: &'static str,
    /// Program and the flags that precede the command string.
    program: String,
    flags: &'static [&'static str],
}

impl Shell {
    pub(super) fn detect() -> Self {
        let bash = |program: &str| Shell {
            name: "bash",
            description: "Run a bash command in the workspace. \
                          Returns stdout, then stderr.",
            program: program.to_string(),
            flags: &["-c"],
        };
        if cfg!(windows) {
            if on_path("bash").is_some() {
                return bash("bash");
            }
            return Shell {
                name: "powershell",
                description: "Run a PowerShell command in the \
                              workspace. Returns stdout, then \
                              stderr.",
                program: "powershell".into(),
                flags: &["-NoProfile", "-NonInteractive", "-Command"],
            };
        }
        bash(if on_path("bash").is_some() {
            "bash"
        } else {
            "sh"
        })
    }

    pub(super) fn argv(&self, command: &str) -> Vec<String> {
        let mut argv = vec![self.program.clone()];
        argv.extend(self.flags.iter().map(|f| f.to_string()));
        argv.push(command.to_string());
        argv
    }
}

fn on_path(program: &str) -> Option<PathBuf> {
    let exts = if cfg!(windows) {
        let pathext = std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE".into());
        std::iter::once(String::new())
            .chain(pathext.split(';').map(str::to_string))
            .collect::<Vec<_>>()
    } else {
        vec![String::new()]
    };
    std::env::split_paths(&std::env::var_os("PATH")?)
        .flat_map(|dir| {
            exts.iter()
                .map(move |e| dir.join(format!("{program}{e}")))
        })
        .find(|p| p.is_file())
}
