//! Windows: links the application icon (`assets/icon/haetty.ico`, committed,
//! see `assets/icon/README.md`) into the executable as icon resource 1, so
//! Explorer, the taskbar, shortcuts and the window (`icon.rs`) show it.

fn main() {
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let ico = manifest.join("../../assets/icon/haetty.ico");
    println!("cargo:rerun-if-changed={}", ico.display());
    println!("cargo:rerun-if-changed=build.rs");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if os != "windows" || env != "gnu" {
        return; // (an MSVC build would use rc.exe; not set up)
    }
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let ico = ico.canonicalize().expect("assets/icon/haetty.ico");
    let rc = out.join("haetty.rc");
    std::fs::write(&rc, format!("1 ICON \"{}\"\n", ico.display().to_string().replace('\\', "/"))).expect("write rc");
    let obj = out.join("haetty-icon.o");
    let windres = std::env::var("WINDRES").unwrap_or_else(|_| "x86_64-w64-mingw32-windres".into());
    let ok = std::process::Command::new(&windres)
        .arg("-i")
        .arg(&rc)
        .arg("-O")
        .arg("coff")
        .arg("-o")
        .arg(&obj)
        .status()
        .is_ok_and(|s| s.success());
    if ok {
        println!("cargo:rustc-link-arg-bins={}", obj.display());
    } else {
        println!("cargo:warning=windres ({windres}) not available: haetty.exe gets no icon resource");
    }
}
