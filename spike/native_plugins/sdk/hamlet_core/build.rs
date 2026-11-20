//! Names the library by `@rpath`, so that every plugin asks for the
//! copy the app has loaded, not the one in its own build folder.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS")
        .is_ok_and(|os| os == "macos")
    {
        println!(
            "cargo::rustc-link-arg=-Wl,-install_name,@rpath/\
             libhamlet_core.dylib"
        );
    }
}
