//! Windows only: embed the app icon and version info (VERSIONINFO) into `keep-launcher.exe`.
//!
//! On every other target this does nothing. A missing resource compiler is a warning, so a
//! cross-compile from macOS or Linux still links, unless `KEEP_LAUNCHER_REQUIRE_WINRES=1` (set
//! by the release packaging) turns it into an error.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../assets/app-icon/keep-launcher.ico");
    println!("cargo:rerun-if-env-changed=KEEP_LAUNCHER_REQUIRE_WINRES");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../../assets/app-icon/keep-launcher.ico")
        .set("ProductName", "MoneyKeep Launcher")
        .set("FileDescription", "MoneyKeep Launcher")
        .set("CompanyName", "MoneyKeep Project")
        .set("LegalCopyright", "Copyright (c) the MoneyKeep Launcher authors. MIT OR Apache-2.0.")
        .set("OriginalFilename", "keep-launcher.exe")
        .set("InternalName", "keep-launcher");
    let (four, packed) = windows_version();
    res.set_version_info(winresource::VersionInfo::FILEVERSION, packed)
        .set_version_info(winresource::VersionInfo::PRODUCTVERSION, packed)
        .set("FileVersion", &four)
        .set("ProductVersion", &std::env::var("CARGO_PKG_VERSION").unwrap_or_default());
    if let Err(e) = res.compile() {
        if std::env::var_os("KEEP_LAUNCHER_REQUIRE_WINRES").is_some() {
            println!("cargo:warning=embedding Windows resources failed: {e}");
            std::process::exit(1);
        }
        println!("cargo:warning=keep-launcher.exe built without icon/version resources: {e}");
    }
}

/// The package version in Windows' four numeric parts, as text and packed for VERSIONINFO: a
/// pre-release's trailing number goes last (0.1.0-alpha.1 -> 0.1.0.1), anything else gets .0,
/// the same rule as the MSI's ProductVersion (`packaging/windows/package.ps1`).
fn windows_version() -> (String, u64) {
    let part = |name: &str| -> u64 { std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(0).min(0xffff) };
    let revision = std::env::var("CARGO_PKG_VERSION_PRE")
        .ok()
        .and_then(|pre| pre.rsplit('.').next().and_then(|n| n.parse::<u64>().ok()))
        .filter(|n| *n <= 0xffff)
        .unwrap_or(0);
    let (major, minor, patch) = (part("CARGO_PKG_VERSION_MAJOR"), part("CARGO_PKG_VERSION_MINOR"), part("CARGO_PKG_VERSION_PATCH"));
    (format!("{major}.{minor}.{patch}.{revision}"), (major << 48) | (minor << 32) | (patch << 16) | revision)
}
