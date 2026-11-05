//! Windows: puts the (placeholder) application icon into the executable as
//! a resource, so Explorer, the taskbar and shortcuts show it. The icon is
//! drawn by `src/icon.rs` — the one place to change it.

#[allow(dead_code)]
#[path = "src/icon.rs"]
mod icon;

fn main() {
    println!("cargo:rerun-if-changed=src/icon.rs");
    println!("cargo:rerun-if-changed=build.rs");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if os != "windows" || env != "gnu" {
        return; // (an MSVC build would use rc.exe; not set up)
    }
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    // Up to 64 px: the PNGs are stored uncompressed, and a 256 px entry
    // alone would add 262 KB to the executable (Explorer scales up).
    let ico = out.join("haetty.ico");
    std::fs::write(&ico, icon::ico(&[16, 24, 32, 48, 64])).expect("write ico");
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
