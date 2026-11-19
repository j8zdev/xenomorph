use std::path::PathBuf;
use std::process::Command;

fn main() {
    let config = slint_build::CompilerConfiguration::new().with_style("fluent".into());
    slint_build::compile_with_config("ui/app.slint", config).expect("Slint build failed");

    // Windows: give the .exe its icon. The resource is compiled with `zig rc`,
    // which is already part of this project's toolchain.
    println!("cargo:rerun-if-changed=assets/app.rc");
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let res = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("app.res");
        let status = Command::new("zig")
            .current_dir("assets")
            .args(["rc", "/fo"])
            .arg(&res)
            .arg("app.rc")
            .status();
        match status {
            Ok(s) if s.success() => println!("cargo:rustc-link-arg-bins={}", res.display()),
            _ => println!("cargo:warning=zig rc is not available: the .exe will have no icon"),
        }
    }
}
