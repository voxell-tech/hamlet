//! Finding Git Bash on Windows. A search by name would take
//! `System32\bash.exe`, the WSL launcher, before any folder on
//! `PATH`, so the server looks where Git for Windows installs it.

/// What the search reads of the machine, so a test can give a fake
/// one.
pub(crate) trait Surroundings {
    /// The variable `name` of this process's environment.
    fn var(&self, name: &str) -> Option<String>;
    fn is_file(&self, path: &str) -> bool;
}

/// This machine.
#[cfg(windows)]
pub(crate) struct Machine;

#[cfg(windows)]
impl Surroundings for Machine {
    fn var(&self, name: &str) -> Option<String> {
        std::env::var(name).ok().filter(|value| !value.is_empty())
    }

    fn is_file(&self, path: &str) -> bool {
        std::fs::metadata(path).is_ok_and(|meta| meta.is_file())
    }
}

/// Git Bash's `bin\bash.exe`: beside the `cmd\git.exe` on `PATH`,
/// else in the folders Git for Windows installs to. Nothing under
/// `%SystemRoot%`.
pub(crate) fn git_bash(around: &impl Surroundings) -> Option<String> {
    let path = around.var("PATH").unwrap_or_default();
    let beside_git = path.split(';').filter_map(|dir| {
        let dir = dir.trim_end_matches('\\');
        let install = dir.strip_suffix(r"\cmd")?;
        around
            .is_file(&format!(r"{dir}\git.exe"))
            .then(|| install.to_owned())
    });
    let defaults = [
        around.var("ProgramFiles").map(|dir| format!(r"{dir}\Git")),
        around
            .var("LOCALAPPDATA")
            .map(|dir| format!(r"{dir}\Programs\Git")),
    ];
    let system = around
        .var("SystemRoot")
        .map(|root| format!(r"{}\", root.to_lowercase()));
    beside_git
        .chain(defaults.into_iter().flatten())
        .map(|install| format!(r"{install}\bin\bash.exe"))
        .filter(|bash| {
            system.as_ref().is_none_or(|system| {
                !bash.to_lowercase().starts_with(system.as_str())
            })
        })
        .find(|bash| around.is_file(bash))
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::*;

    /// A machine with these variables and these files.
    struct Fake {
        vars: BTreeMap<&'static str, &'static str>,
        files: BTreeSet<&'static str>,
    }

    impl Surroundings for Fake {
        fn var(&self, name: &str) -> Option<String> {
            self.vars.get(name).map(|value| (*value).to_owned())
        }

        fn is_file(&self, path: &str) -> bool {
            self.files.contains(path)
        }
    }

    fn fake(
        vars: &[(&'static str, &'static str)],
        files: &[&'static str],
    ) -> Fake {
        let mut all = BTreeMap::from([
            ("SystemRoot", r"C:\Windows"),
            ("ProgramFiles", r"C:\Program Files"),
        ]);
        all.extend(vars.iter().copied());
        Fake {
            vars: all,
            files: files.iter().copied().collect(),
        }
    }

    #[test]
    fn finds_git_bash_beside_cmd_git() {
        let machine = fake(
            &[("PATH", r"C:\Windows\System32;D:\Tools\Git\cmd")],
            &[
                r"C:\Windows\System32\bash.exe",
                r"D:\Tools\Git\cmd\git.exe",
                r"D:\Tools\Git\bin\bash.exe",
                r"C:\Program Files\Git\bin\bash.exe",
            ],
        );
        assert_eq!(
            git_bash(&machine).as_deref(),
            Some(r"D:\Tools\Git\bin\bash.exe")
        );
    }

    #[test]
    fn finds_git_bash_in_the_default_folder() {
        let machine = fake(
            &[("PATH", r"C:\Windows\System32")],
            &[r"C:\Program Files\Git\bin\bash.exe"],
        );
        assert_eq!(
            git_bash(&machine).as_deref(),
            Some(r"C:\Program Files\Git\bin\bash.exe")
        );
    }

    #[test]
    fn refuses_bash_under_system_root() {
        let machine = fake(
            &[("PATH", r"C:\WINDOWS\cmd"), ("ProgramFiles", "")],
            &[r"C:\WINDOWS\cmd\git.exe", r"C:\WINDOWS\bin\bash.exe"],
        );
        assert_eq!(git_bash(&machine), None);
    }

    #[test]
    fn no_git_bash_means_no_bash() {
        let machine = fake(
            &[("PATH", r"C:\Windows\System32")],
            &[r"C:\Windows\System32\bash.exe"],
        );
        assert_eq!(git_bash(&machine), None);
    }
}
