//! Platforms, package formats, and choosing the right release asset.
//!
//! Every MoneyKeep App release names its files `<app>-<version>-<os>-<arch>[-<kind>].<ext>`, for
//! example `moneykeep-0.5.0-windows-x64.msi`, `moneykeep-0.1.0-alpha.1-windows-x64-setup.exe` or
//! `moneykeep-0.5.0-linux-aarch64.AppImage`.
//!
//! Labels are English. The UI translates [`PackageKind::label`]; OS and architecture names are
//! shown as they are.

use serde::{Deserialize, Serialize};

use crate::release::{Asset, Release};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Os {
    Windows,
    MacOs,
    Linux,
    FreeBsd,
}

impl Os {
    pub fn label(self) -> &'static str {
        match self {
            Self::Windows => "Windows",
            Self::MacOs => "macOS",
            Self::Linux => "Linux",
            Self::FreeBsd => "FreeBSD",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Arch {
    X64,
    X86,
    Arm64,
    Riscv64,
}

impl Arch {
    pub fn label(self) -> &'static str {
        match self {
            Self::X64 => "x64",
            Self::X86 => "x86",
            Self::Arm64 => "ARM64",
            Self::Riscv64 => "RISC-V",
        }
    }
}

/// A package format a release ships.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PackageKind {
    /// Windows Installer package (per user by default; an existing per-machine copy stays per-machine).
    Msi,
    /// Windows setup program (`-setup.exe`, NSIS), per machine. The launcher runs it silently
    /// (`/S`); the setup asks for administrator permission itself.
    SetupExe,
    /// Windows zip with the exe at the top level.
    PortableZip,
    /// macOS disk image with the `.app` bundle.
    Dmg,
    AppImage,
    Deb,
    Rpm,
    /// Arch Linux package (`.pkg.tar.zst`).
    ArchPkg,
    Flatpak,
    /// Linux or FreeBSD tarball (`bin/<app>`, `share/…`).
    TarGz,
}

impl PackageKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Msi => "Installer (.msi)",
            Self::SetupExe => "Installer (.exe)",
            Self::PortableZip => "Portable (.zip)",
            Self::Dmg => "Disk image (.dmg)",
            Self::AppImage => "AppImage",
            Self::Deb => "Debian package (.deb)",
            Self::Rpm => "RPM package (.rpm)",
            Self::ArchPkg => "Arch package (.pkg.tar.zst)",
            Self::Flatpak => "Flatpak bundle",
            Self::TarGz => "Tarball (.tar.gz)",
        }
    }

    fn suffix(self) -> &'static str {
        match self {
            Self::Msi => ".msi",
            Self::SetupExe => "-setup.exe",
            Self::PortableZip => "-portable.zip",
            Self::Dmg => ".dmg",
            Self::AppImage => ".AppImage",
            Self::Deb => ".deb",
            Self::Rpm => ".rpm",
            Self::ArchPkg => ".pkg.tar.zst",
            Self::Flatpak => ".flatpak",
            Self::TarGz => ".tar.gz",
        }
    }
}

/// Where the launcher is running (or, on the web, the visitor's machine).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Platform {
    pub os: Os,
    pub arch: Arch,
}

impl Platform {
    /// The platform this binary was built for, or `None` for wasm and unsupported targets.
    pub fn current() -> Option<Self> {
        let os = if cfg!(target_os = "windows") {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::MacOs
        } else if cfg!(target_os = "linux") {
            Os::Linux
        } else if cfg!(target_os = "freebsd") {
            Os::FreeBsd
        } else {
            return None;
        };
        let arch = if cfg!(target_arch = "x86_64") {
            Arch::X64
        } else if cfg!(target_arch = "x86") {
            Arch::X86
        } else if cfg!(target_arch = "aarch64") {
            Arch::Arm64
        } else if cfg!(target_arch = "riscv64") {
            Arch::Riscv64
        } else {
            return None;
        };
        Some(Self { os, arch })
    }

    pub fn label(self) -> String {
        match self.os {
            Os::MacOs => "macOS".into(),
            os => format!("{} {}", os.label(), self.arch.label()),
        }
    }

    /// Asset platform tokens to try, best first. Windows on ARM and 64-bit Windows can fall back
    /// to emulated builds; macOS prefers universal, then native builds. Apple silicon can fall
    /// back to Intel builds, but Intel Macs cannot run ARM builds.
    fn tokens(self) -> &'static [&'static str] {
        match (self.os, self.arch) {
            (Os::Windows, Arch::X64) => &["windows-x64", "windows-x86"],
            (Os::Windows, Arch::X86) => &["windows-x86"],
            (Os::Windows, Arch::Arm64) => &["windows-arm64", "windows-x64", "windows-x86"],
            (Os::Windows, _) => &[],
            (Os::MacOs, Arch::X64) => &["macos-universal", "macos-x86_64"],
            (Os::MacOs, Arch::Arm64) => &["macos-universal", "macos-aarch64", "macos-arm64", "macos-x86_64"],
            (Os::MacOs, _) => &[],
            (Os::Linux, Arch::X64) => &["linux-x86_64"],
            (Os::Linux, Arch::Arm64) => &["linux-aarch64"],
            (Os::Linux, Arch::Riscv64) => &["linux-riscv64"],
            (Os::Linux, Arch::X86) => &[],
            (Os::FreeBsd, Arch::X64) => &["freebsd-x86_64"],
            (Os::FreeBsd, Arch::Arm64) => &["freebsd-aarch64"],
            (Os::FreeBsd, _) => &[],
        }
    }

    /// Package formats the launcher installs on this OS, in order of preference. On Windows an
    /// MSI comes first, then a setup program, then the portable zip.
    pub fn default_kinds(self) -> &'static [PackageKind] {
        match self.os {
            Os::Windows => &[PackageKind::Msi, PackageKind::SetupExe, PackageKind::PortableZip],
            Os::MacOs => &[PackageKind::Dmg],
            Os::Linux => &[PackageKind::AppImage, PackageKind::TarGz],
            Os::FreeBsd => &[PackageKind::TarGz],
        }
    }

    /// Every format offered as a manual download on this OS (the web build's download menu).
    pub fn download_kinds(self) -> &'static [PackageKind] {
        match self.os {
            Os::Windows => &[PackageKind::Msi, PackageKind::SetupExe, PackageKind::PortableZip],
            Os::MacOs => &[PackageKind::Dmg],
            Os::Linux => &[PackageKind::AppImage, PackageKind::Deb, PackageKind::Rpm, PackageKind::ArchPkg, PackageKind::Flatpak, PackageKind::TarGz],
            Os::FreeBsd => &[PackageKind::TarGz],
        }
    }
}

/// What an asset name says about itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetName {
    pub stem: String,
    pub version: String,
    pub token: String,
    pub kind: PackageKind,
}

impl AssetName {
    /// The architecture the asset is built for: the last part of its token (`windows-x86` → x86).
    /// `None` for a token that names none (`macos-universal`).
    pub fn arch(&self) -> Option<Arch> {
        match self.token.rsplit('-').next()? {
            "x64" | "x86_64" => Some(Arch::X64),
            "x86" => Some(Arch::X86),
            "arm64" | "aarch64" => Some(Arch::Arm64),
            "riscv64" => Some(Arch::Riscv64),
            _ => None,
        }
    }
}

/// Reads `<stem>-<version>-<os>-<arch>[-portable|-setup].<ext>`; `None` for checksums, zsync
/// files, CLI and web bundles, or anything else.
pub fn parse_asset_name(name: &str, stems: &[&str]) -> Option<AssetName> {
    const KINDS: [PackageKind; 10] = [
        PackageKind::PortableZip,
        PackageKind::SetupExe,
        PackageKind::TarGz,
        PackageKind::Msi,
        PackageKind::Dmg,
        PackageKind::AppImage,
        PackageKind::Deb,
        PackageKind::Rpm,
        PackageKind::ArchPkg,
        PackageKind::Flatpak,
    ];
    let (kind, base) = KINDS.iter().find_map(|k| name.strip_suffix(k.suffix()).map(|b| (*k, b)))?;
    for stem in stems {
        let Some(rest) = base.strip_prefix(stem).and_then(|r| r.strip_prefix('-')) else { continue };
        // rest = <version>-<os>-<arch>; os and arch never contain '-', versions may.
        let mut parts = rest.rsplitn(3, '-');
        let (arch, os, version) = (parts.next()?, parts.next()?, parts.next()?);
        if crate::version::parse(version).is_none() || !version.starts_with(|c: char| c.is_ascii_digit()) {
            continue;
        }
        return Some(AssetName { stem: (*stem).to_string(), version: version.to_string(), token: format!("{os}-{arch}"), kind });
    }
    None
}

/// The best asset of `release` for `platform`, trying `kinds` in order and, within a kind, the
/// platform's tokens in order (native before emulated).
pub fn pick_asset<'a>(release: &'a Release, stems: &[&str], platform: Platform, kinds: &[PackageKind]) -> Option<(&'a Asset, PackageKind)> {
    let parsed: Vec<(&Asset, AssetName)> = release.assets.iter().filter_map(|a| parse_asset_name(&a.name, stems).map(|n| (a, n))).collect();
    for kind in kinds {
        for token in platform.tokens() {
            if let Some((asset, _)) = parsed.iter().find(|(_, n)| n.kind == *kind && n.token == *token) {
                return Some((asset, *kind));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(names: &[&str]) -> Release {
        Release {
            assets: names.iter().map(|n| Asset { name: (*n).into(), size: 1, url: format!("https://x/{n}"), sha256: None }).collect(),
            ..Release::default()
        }
    }

    const PHOTO: &[&str] = &[
        "moneykeep-0.5.0-freebsd-x86_64.tar.gz",
        "moneykeep-0.5.0-linux-aarch64.AppImage",
        "moneykeep-0.5.0-linux-aarch64.AppImage.zsync",
        "moneykeep-0.5.0-linux-aarch64.deb",
        "moneykeep-0.5.0-linux-riscv64.tar.gz",
        "moneykeep-0.5.0-linux-x86_64.AppImage",
        "moneykeep-0.5.0-linux-x86_64.tar.gz",
        "moneykeep-0.5.0-macos-universal.dmg",
        "moneykeep-0.5.0-windows-x64-portable.zip",
        "moneykeep-0.5.0-windows-x64.msi",
        "moneykeep-0.5.0-windows-x86.msi",
        "moneykeep-cli-0.5.0-macos-universal.zip",
        "moneykeep-web-0.5.0.zip",
        "SHA256SUMS.txt",
    ];

    #[test]
    fn macos_prefers_the_published_universal_build() {
        // MoneyKeep v0.9.0's GitHub metadata, trimmed to its desktop DMG.
        let mut r =
            crate::release::parse_github_release(include_str!("../tests/fixtures/moneykeep-release.json"), "moneykeepproject/moneykeep").unwrap();
        // Split builds must not outrank the published universal DMG, even when listed first.
        r.assets.splice(0..0, release(&["moneykeep-0.9.0-macos-arm64.dmg", "moneykeep-0.9.0-macos-x86_64.dmg"]).assets);
        for arch in [Arch::X64, Arch::Arm64] {
            let (asset, kind) = pick_asset(&r, &["moneykeep"], Platform { os: Os::MacOs, arch }, &[PackageKind::Dmg]).unwrap();
            assert_eq!(asset.name, "moneykeep-0.9.0-macos-universal.dmg");
            assert_eq!(kind, PackageKind::Dmg);
        }
    }

    #[test]
    fn loankeep_selects_published_desktop_assets() {
        let release =
            crate::release::parse_github_release(include_str!("../tests/fixtures/loankeep-release.json"), "moneykeepproject/loankeep").unwrap();
        for (os, kind, expected) in [
            (Os::Windows, PackageKind::Msi, "loankeep-0.1.0-windows-x64.msi"),
            (Os::MacOs, PackageKind::Dmg, "loankeep-0.1.0-macos-universal.dmg"),
            (Os::Linux, PackageKind::Flatpak, "loankeep-0.1.0-linux-x86_64.flatpak"),
        ] {
            let platform = Platform { os, arch: Arch::X64 };
            let (asset, selected) = pick_asset(&release, &["loankeep"], platform, &[kind]).unwrap();
            assert_eq!(asset.name, expected);
            assert_eq!(selected, kind);
        }
    }

    #[test]
    fn freebsd_picks_the_tarball_for_the_processor() {
        // PayKeep v0.10.0's GitHub metadata, trimmed to its FreeBSD tarballs.
        let r = crate::release::parse_github_release(include_str!("../tests/fixtures/paykeep-release.json"), "moneykeepproject/paykeep").unwrap();
        let pick = |arch| {
            let platform = Platform { os: Os::FreeBsd, arch };
            pick_asset(&r, &["paykeep"], platform, platform.default_kinds()).map(|(a, k)| (a.name.as_str(), k))
        };
        assert_eq!(pick(Arch::X64), Some(("paykeep-0.10.0-freebsd-x86_64.tar.gz", PackageKind::TarGz)));
        assert_eq!(pick(Arch::Arm64), Some(("paykeep-0.10.0-freebsd-aarch64.tar.gz", PackageKind::TarGz)));
        for arch in [Arch::X86, Arch::Riscv64] {
            assert_eq!(pick(arch), None);
        }
        // An x86_64 machine never takes the ARM tarball, nor an ARM machine the x86_64 one.
        let only_arm = release(&["paykeep-0.10.0-freebsd-aarch64.tar.gz"]);
        assert_eq!(pick_asset(&only_arm, &["paykeep"], Platform { os: Os::FreeBsd, arch: Arch::X64 }, &[PackageKind::TarGz]), None);
        let only_x64 = release(&["paykeep-0.10.0-freebsd-x86_64.tar.gz"]);
        assert_eq!(pick_asset(&only_x64, &["paykeep"], Platform { os: Os::FreeBsd, arch: Arch::Arm64 }, &[PackageKind::TarGz]), None);
    }

    #[test]
    fn macos_split_builds_match_the_processor() {
        let r = release(&["moneykeep-0.5.0-macos-arm64.dmg", "moneykeep-0.5.0-macos-x86_64.dmg"]);
        let pick = |arch| pick_asset(&r, &["moneykeep"], Platform { os: Os::MacOs, arch }, &[PackageKind::Dmg]).map(|(a, _)| a.name.as_str());
        assert_eq!(pick(Arch::X64), Some("moneykeep-0.5.0-macos-x86_64.dmg"));
        assert_eq!(pick(Arch::Arm64), Some("moneykeep-0.5.0-macos-arm64.dmg"));
    }

    #[test]
    fn intel_macos_never_selects_an_arm_only_build() {
        for token in ["macos-aarch64", "macos-arm64"] {
            let name = format!("moneykeep-0.5.0-{token}.dmg");
            let r = release(&[&name]);
            assert_eq!(pick_asset(&r, &["moneykeep"], Platform { os: Os::MacOs, arch: Arch::X64 }, &[PackageKind::Dmg]), None);
            assert_eq!(
                pick_asset(&r, &["moneykeep"], Platform { os: Os::MacOs, arch: Arch::Arm64 }, &[PackageKind::Dmg]).map(|(a, _)| a.name.as_str()),
                Some(name.as_str())
            );
        }
    }

    #[test]
    fn apple_silicon_keeps_the_intel_fallback() {
        let r = release(&["moneykeep-0.5.0-macos-x86_64.dmg"]);
        assert_eq!(
            pick_asset(&r, &["moneykeep"], Platform { os: Os::MacOs, arch: Arch::Arm64 }, &[PackageKind::Dmg]).map(|(a, _)| a.name.as_str()),
            Some("moneykeep-0.5.0-macos-x86_64.dmg")
        );
        for arch in [Arch::X86, Arch::Riscv64] {
            assert_eq!(pick_asset(&r, &["moneykeep"], Platform { os: Os::MacOs, arch }, &[PackageKind::Dmg]), None);
        }
    }

    #[test]
    fn asset_names_carry_their_architecture() {
        let arch = |name: &str| parse_asset_name(name, &["moneykeep"]).unwrap().arch();
        assert_eq!(arch("moneykeep-0.5.0-windows-x64.msi"), Some(Arch::X64));
        assert_eq!(arch("moneykeep-0.5.0-windows-x86.msi"), Some(Arch::X86));
        assert_eq!(arch("moneykeep-0.5.0-linux-aarch64.tar.gz"), Some(Arch::Arm64));
        assert_eq!(arch("moneykeep-0.5.0-macos-universal.dmg"), None);
    }

    #[test]
    fn reads_asset_names() {
        let n = parse_asset_name("moneykeep-0.2.0-rc.1-windows-arm64-portable.zip", &["moneykeep"]).unwrap();
        assert_eq!((n.version.as_str(), n.token.as_str(), n.kind), ("0.2.0-rc.1", "windows-arm64", PackageKind::PortableZip));
        let n = parse_asset_name("bookkeep-0.2.1-linux-x86_64.tar.gz", &["ledgerkeep", "bookkeep"]).unwrap();
        assert_eq!((n.stem.as_str(), n.kind), ("bookkeep", PackageKind::TarGz));
        for other in
            ["moneykeep-cli-0.5.0-macos-universal.zip", "moneykeep-web-0.5.0.zip", "SHA256SUMS.txt", "moneykeep-0.5.0-linux-x86_64.AppImage.zsync"]
        {
            assert_eq!(parse_asset_name(other, &["moneykeep"]), None, "{other}");
        }
        // ReceiptKeep also ships its collaboration server; that tarball is not the app.
        assert_eq!(parse_asset_name("receiptkeep-server-0.5.1-linux-x86_64.tar.gz", &["receiptkeep"]), None);
        assert!(parse_asset_name("receiptkeep-0.5.1-linux-x86_64.tar.gz", &["receiptkeep"]).is_some());
    }

    #[test]
    fn picks_the_native_build_first_and_falls_back_to_emulation() {
        let r = release(PHOTO);
        let pick = |os, arch, kinds: &[PackageKind]| pick_asset(&r, &["moneykeep"], Platform { os, arch }, kinds).map(|(a, _)| a.name.clone());
        assert_eq!(pick(Os::MacOs, Arch::Arm64, &[PackageKind::Dmg]).as_deref(), Some("moneykeep-0.5.0-macos-universal.dmg"));
        assert_eq!(pick(Os::Windows, Arch::X64, &[PackageKind::Msi]).as_deref(), Some("moneykeep-0.5.0-windows-x64.msi"));
        // No arm64 MSI in this release: Windows on ARM runs the x64 build.
        assert_eq!(pick(Os::Windows, Arch::Arm64, &[PackageKind::Msi]).as_deref(), Some("moneykeep-0.5.0-windows-x64.msi"));
        assert_eq!(pick(Os::Linux, Arch::Arm64, &[PackageKind::AppImage]).as_deref(), Some("moneykeep-0.5.0-linux-aarch64.AppImage"));
        assert_eq!(pick(Os::Linux, Arch::Arm64, &[PackageKind::TarGz]), None);
        assert_eq!(pick(Os::Linux, Arch::Riscv64, &[PackageKind::TarGz]).as_deref(), Some("moneykeep-0.5.0-linux-riscv64.tar.gz"));
        assert_eq!(pick(Os::FreeBsd, Arch::X64, &[PackageKind::TarGz]).as_deref(), Some("moneykeep-0.5.0-freebsd-x86_64.tar.gz"));
        // Preference order across kinds.
        assert_eq!(
            pick(Os::Windows, Arch::X64, &[PackageKind::PortableZip, PackageKind::Msi]).as_deref(),
            Some("moneykeep-0.5.0-windows-x64-portable.zip")
        );
        assert_eq!(pick(Os::Windows, Arch::X86, &[PackageKind::PortableZip, PackageKind::Msi]).as_deref(), Some("moneykeep-0.5.0-windows-x86.msi"));

        let p_riscv = Platform { os: Os::Linux, arch: Arch::Riscv64 };
        assert_eq!(
            pick_asset(&r, &["moneykeep"], p_riscv, p_riscv.default_kinds()).map(|(a, _)| a.name.clone()).as_deref(),
            Some("moneykeep-0.5.0-linux-riscv64.tar.gz")
        );
        assert_eq!(
            pick_asset(&r, &["moneykeep"], p_riscv, p_riscv.download_kinds()).map(|(a, _)| a.name.clone()).as_deref(),
            Some("moneykeep-0.5.0-linux-riscv64.tar.gz")
        );
        let r_riscv_deb =
            release(&["moneykeep-0.5.0-linux-riscv64.tar.gz", "moneykeep-0.5.0-linux-riscv64.deb", "moneykeep-0.5.0-linux-riscv64.rpm"]);
        assert_eq!(
            pick_asset(&r_riscv_deb, &["moneykeep"], p_riscv, p_riscv.download_kinds()).map(|(a, _)| a.name.clone()).as_deref(),
            Some("moneykeep-0.5.0-linux-riscv64.deb")
        );
    }

    /// The alpha releases ship a Windows NSIS setup (`-setup.exe`) and a `SHA256SUMS.txt`.
    const ALPHA: &[&str] = &["moneykeep-0.1.0-alpha.1-windows-x64-setup.exe", "SHA256SUMS.txt"];

    #[test]
    fn reads_setup_program_names() {
        let n = parse_asset_name("moneykeep-0.1.0-alpha.1-windows-x64-setup.exe", &["moneykeep"]).unwrap();
        assert_eq!(
            (n.stem.as_str(), n.version.as_str(), n.token.as_str(), n.kind),
            ("moneykeep", "0.1.0-alpha.1", "windows-x64", PackageKind::SetupExe)
        );
        assert_eq!(n.arch(), Some(Arch::X64));
        let n = parse_asset_name("form1099keep-0.1.0-alpha.1-windows-x64-setup.exe", &["form1099keep"]).unwrap();
        assert_eq!((n.version.as_str(), n.kind), ("0.1.0-alpha.1", PackageKind::SetupExe));
        // Other names that end in .exe are not setups the launcher installs: the old setup name,
        // a bare program, and a setup without a platform.
        for other in [
            "MoneyKeep-Setup-0.1.0-alpha.1.exe",
            "moneykeep-0.1.0-alpha.1-windows-x64.exe",
            "moneykeep-0.1.0-alpha.1-setup.exe",
            "moneykeep-cli-0.1.0-alpha.1-windows-x64-setup.exe",
        ] {
            assert_eq!(parse_asset_name(other, &["moneykeep"]), None, "{other}");
        }
        assert_eq!(PackageKind::SetupExe.label(), "Installer (.exe)");
    }

    #[test]
    fn windows_prefers_the_msi_then_the_setup_then_the_portable_zip() {
        let windows = Platform { os: Os::Windows, arch: Arch::X64 };
        let pick = |names: &[&str]| pick_asset(&release(names), &["moneykeep"], windows, windows.default_kinds()).map(|(a, k)| (a.name.clone(), k));
        assert_eq!(pick(ALPHA), Some(("moneykeep-0.1.0-alpha.1-windows-x64-setup.exe".into(), PackageKind::SetupExe)));
        let all = ["moneykeep-0.5.0-windows-x64-portable.zip", "moneykeep-0.5.0-windows-x64-setup.exe", "moneykeep-0.5.0-windows-x64.msi"];
        assert_eq!(pick(&all), Some(("moneykeep-0.5.0-windows-x64.msi".into(), PackageKind::Msi)));
        assert_eq!(pick(&all[..2]), Some(("moneykeep-0.5.0-windows-x64-setup.exe".into(), PackageKind::SetupExe)));
        assert_eq!(pick(&all[..1]), Some(("moneykeep-0.5.0-windows-x64-portable.zip".into(), PackageKind::PortableZip)));
        // Windows on ARM runs the x64 setup; no other system takes it.
        let arm = Platform { os: Os::Windows, arch: Arch::Arm64 };
        assert_eq!(pick_asset(&release(ALPHA), &["moneykeep"], arm, arm.default_kinds()).map(|(_, k)| k), Some(PackageKind::SetupExe));
        let x86 = Platform { os: Os::Windows, arch: Arch::X86 };
        assert_eq!(pick_asset(&release(ALPHA), &["moneykeep"], x86, x86.default_kinds()), None);
        for os in [Os::MacOs, Os::Linux, Os::FreeBsd] {
            let p = Platform { os, arch: Arch::X64 };
            assert_eq!(pick_asset(&release(ALPHA), &["moneykeep"], p, p.download_kinds()), None, "{os:?}");
        }
        assert!(windows.download_kinds().contains(&PackageKind::SetupExe));
    }

    #[test]
    fn strictly_matches_architecture_for_all_linux_package_formats() {
        let multi_arch_release = release(&[
            "moneykeep-0.5.0-linux-aarch64.AppImage",
            "moneykeep-0.5.0-linux-x86_64.AppImage",
            "moneykeep-0.5.0-linux-aarch64.deb",
            "moneykeep-0.5.0-linux-x86_64.deb",
            "moneykeep-0.5.0-linux-aarch64.rpm",
            "moneykeep-0.5.0-linux-x86_64.rpm",
            "moneykeep-0.5.0-linux-aarch64.pkg.tar.zst",
            "moneykeep-0.5.0-linux-x86_64.pkg.tar.zst",
            "moneykeep-0.5.0-linux-aarch64.flatpak",
            "moneykeep-0.5.0-linux-x86_64.flatpak",
            "moneykeep-0.5.0-linux-aarch64.tar.gz",
            "moneykeep-0.5.0-linux-x86_64.tar.gz",
        ]);

        let p_x64 = Platform { os: Os::Linux, arch: Arch::X64 };
        let p_arm64 = Platform { os: Os::Linux, arch: Arch::Arm64 };

        for kind in [PackageKind::AppImage, PackageKind::Deb, PackageKind::Rpm, PackageKind::ArchPkg, PackageKind::Flatpak, PackageKind::TarGz] {
            let pick_x64 = pick_asset(&multi_arch_release, &["moneykeep"], p_x64, &[kind]).map(|(a, _)| a.name.clone());
            assert!(pick_x64.as_deref().unwrap().contains("linux-x86_64"), "Expected x86_64 asset for {kind:?}, got {pick_x64:?}");

            let pick_arm64 = pick_asset(&multi_arch_release, &["moneykeep"], p_arm64, &[kind]).map(|(a, _)| a.name.clone());
            assert!(pick_arm64.as_deref().unwrap().contains("linux-aarch64"), "Expected aarch64 asset for {kind:?}, got {pick_arm64:?}");
        }

        // On x86_64 Linux, an aarch64-only asset must NEVER match or fall back
        let arm_only_release = release(&[
            "moneykeep-0.5.0-linux-aarch64.AppImage",
            "moneykeep-0.5.0-linux-aarch64.deb",
            "moneykeep-0.5.0-linux-aarch64.rpm",
            "moneykeep-0.5.0-linux-aarch64.pkg.tar.zst",
            "moneykeep-0.5.0-linux-aarch64.flatpak",
            "moneykeep-0.5.0-linux-aarch64.tar.gz",
        ]);
        for kind in [PackageKind::AppImage, PackageKind::Deb, PackageKind::Rpm, PackageKind::ArchPkg, PackageKind::Flatpak, PackageKind::TarGz] {
            assert_eq!(pick_asset(&arm_only_release, &["moneykeep"], p_x64, &[kind]), None, "x86_64 must never match an aarch64 asset for {kind:?}");
        }
    }
}
