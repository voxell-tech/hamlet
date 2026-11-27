//! Finds the shared libraries beside the app, where a release puts
//! them.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS")
        .is_ok_and(|os| os == "macos")
    {
        println!(
            "cargo::rustc-link-arg-bins=-Wl,-rpath,@executable_path"
        );
    }
}
