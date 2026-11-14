//! The stand-in Node host, the example `link_child`, as a host's
//! child.

use std::path::PathBuf;

use hamlet_host::child::ChildOptions;

/// The stand-in, in the `examples` folder of the profile's folder,
/// which holds the test at some depth: the depth differs between
/// cargo's build layouts.
pub fn stand_in() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    exe.ancestors()
        .map(|folder| {
            folder.join("examples").join(format!(
                "link_child{}",
                std::env::consts::EXE_SUFFIX
            ))
        })
        .find(|path| path.exists())
        .unwrap_or_else(|| {
            panic!(
                "no stand-in Node host above {}: build it with \
                 `cargo build -p hamlet_host --examples`",
                exe.display()
            )
        })
}

/// The stand-in run with `args`, holding `agent`, as the child.
pub fn child(args: &[&str]) -> ChildOptions {
    let mut argv = vec![stand_in().to_str().unwrap().to_owned()];
    argv.extend(args.iter().map(|arg| (*arg).to_owned()));
    ChildOptions {
        argv,
        kinds: vec!["agent".to_owned()],
    }
}
