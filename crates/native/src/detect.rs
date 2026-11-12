//! Finding MoneyKeep Apps that are already installed, however they got there.
//!
//! - macOS: `<App>.app` in `/Applications` or `~/Applications`, read from its `Info.plist`.
//! - Windows: the Windows Installer's uninstall entries (`MoneyKeep Project`), a setup
//!   program's uninstall entry (`Uninstall\<App>`, same publisher), and portable copies the
//!   launcher unpacked.
//! - Linux: AppImages in `~/Applications` (where the launcher and AppImageLauncher put them),
//!   `~/AppImages` (Gear Lever) and `~/.local/bin`, Debian/RPM packages, Flatpaks, and tarball
//!   installs.
//! - FreeBSD: tarball installs and `/usr/local/bin/<app>`.

use std::path::{Path, PathBuf};

use keep_launcher_core::{InstallSource, Installed, KeepApp, version};

use crate::manifest::Manifest;

/// One look at the system, shared by every app's detection (package lists are read once).
#[derive(Default)]
pub struct Snapshot {
    #[allow(dead_code)]
    packages: Vec<crate::packages::Package>,
}

impl Snapshot {
    pub fn take() -> Self {
        #[cfg(target_os = "linux")]
        {
            Self { packages: linux::packages() }
        }
        #[cfg(not(target_os = "linux"))]
        {
            Self::default()
        }
    }
}

/// What's installed of `app`, best source first.
pub fn detect(app: &KeepApp, manifest: &Manifest, snapshot: &Snapshot) -> Option<Installed> {
    let _ = snapshot;
    if let Some(found) = from_manifest(app, manifest) {
        return Some(found);
    }
    #[cfg(target_os = "macos")]
    return macos::detect(app);
    #[cfg(windows)]
    return windows::detect(app);
    #[cfg(target_os = "linux")]
    return linux::detect(app, snapshot);
    #[cfg(target_os = "freebsd")]
    return freebsd::detect(app);
    #[allow(unreachable_code)]
    None
}

fn from_manifest(app: &KeepApp, manifest: &Manifest) -> Option<Installed> {
    let rec = manifest.apps.get(app.id)?;
    if !rec.path.exists() {
        return None;
    }
    // On macOS the bundle itself is the truth (the app may have updated itself).
    #[cfg(target_os = "macos")]
    if rec.kind == keep_launcher_core::PackageKind::Dmg {
        return macos::read_bundle(app, &rec.path);
    }
    Some(Installed {
        version: version::parse(&rec.version),
        source: InstallSource::Launcher,
        location: rec.path.display().to_string(),
        package: None,
        externally_managed: false,
        origin: None,
        installation: None,
    })
}

/// Whether a file name is an AppImage of the app with asset stems `stems`, and the version its
/// name gives. Matches only the names a release AppImage ends up with: the release's own
/// (`moneykeep-0.6.0-linux-x86_64.AppImage`) and the bare app name Gear Lever renames it to
/// (`moneykeep.appimage`, version unknown). The extension's case doesn't matter.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn appimage_version(name: &str, stems: &[&str]) -> Option<Option<String>> {
    let split = name.len().checked_sub(".appimage".len())?;
    let (base, ext) = (name.get(..split)?, name.get(split..)?);
    if !ext.eq_ignore_ascii_case(".appimage") {
        return None;
    }
    if stems.iter().any(|s| base.eq_ignore_ascii_case(s)) {
        return Some(None);
    }
    let parsed = keep_launcher_core::platform::parse_asset_name(&format!("{base}.AppImage"), stems)?;
    (parsed.kind == keep_launcher_core::PackageKind::AppImage && parsed.token.starts_with("linux-")).then_some(Some(parsed.version))
}

/// Who our Windows Installer packages name as their publisher. A community app's names its own
/// ([`KeepApp::msi_publisher`]).
#[cfg_attr(not(windows), allow(dead_code))]
const PUBLISHER: &str = "MoneyKeep Project";

/// The publisher name packages built before 0.1.0-alpha.1 used. Still recognised, so an install
/// made by one of them is found (and updated in place) after the rename.
#[cfg_attr(not(windows), allow(dead_code))]
const FORMER_PUBLISHER: &str = "MoneyKeep Project Inc";

/// Whether an uninstall entry's `DisplayName` and `Publisher` are `app`'s, under any name its
/// installers have registered.
#[cfg_attr(not(windows), allow(dead_code))]
fn is_installer_entry(app: &KeepApp, display_name: &str, publisher: &str) -> bool {
    let ours = match app.msi_publisher {
        Some(theirs) => publisher == theirs,
        None => publisher == PUBLISHER || publisher == FORMER_PUBLISHER,
    };
    ours && app.names().any(|n| n == display_name)
}

/// Where a Windows install of `app` may keep its exe, in the order to try: the entry's
/// `InstallLocation`, then `<App>` under each install root (the `MoneyKeep` suite folder first, then the
/// root itself, for Program Files and the per-user Programs folder) for each name the app has had,
/// and in each folder `<name>.exe` for each of the app's program names ([`KeepApp::exe_names`]),
/// current first. Doesn't touch the disk.
#[cfg_attr(not(windows), allow(dead_code))]
fn exe_candidates(app: &KeepApp, location: Option<&Path>, program_files: &[PathBuf]) -> Vec<PathBuf> {
    let in_program_files = program_files.iter().flat_map(|root| app.names().map(move |n| root.join(n)));
    let dirs = location.map(Path::to_path_buf).into_iter().chain(in_program_files);
    let names = app.exe_names();
    dirs.flat_map(|d| names.iter().map(move |s| d.join(format!("{s}.exe"))).collect::<Vec<_>>()).collect()
}

/// The folder under `MoneyKeep` that the MSI installs to when it is not the display name: the launcher
/// installs to `MoneyKeep\Launcher`, the apps to `MoneyKeep\<App>`.
#[cfg_attr(not(windows), allow(dead_code))]
fn suite_folders(app: &KeepApp, roots: &[PathBuf]) -> Vec<PathBuf> {
    if app.id != keep_launcher_core::catalog::LAUNCHER_ID {
        return Vec::new();
    }
    roots.iter().map(|root| root.join("MoneyKeep").join("Launcher")).collect()
}

/// The registry keys an MSI records its install folder in (value `InstallDir`). The launcher's package
/// keeps its own key, under the publisher name, and an install made before the publisher was renamed
/// is still found under the former name's key.
#[cfg_attr(not(windows), allow(dead_code))]
fn install_dir_keys(app: &KeepApp) -> Vec<String> {
    let mut keys = vec![format!(r"SOFTWARE\{}", app.name)];
    if app.id == keep_launcher_core::catalog::LAUNCHER_ID {
        keys.push(format!(r"SOFTWARE\{PUBLISHER}\{}", app.name));
        keys.push(format!(r"SOFTWARE\{FORMER_PUBLISHER}\{}", app.name));
    }
    keys
}

/// Version from `<app> --version` output. Binaries print the machine id (`moneykeep 0.7.0`),
/// the display name (`CalcKeep 0.5.0`), or a former id (`bookkeep 0.2.1` for LedgerKeep).
#[cfg_attr(not(any(target_os = "linux", target_os = "freebsd")), allow(dead_code))]
fn parse_cli_version(line: &str, app: &KeepApp) -> Option<version::Version> {
    let line = line.trim();
    for prefix in app.stems().chain(std::iter::once(app.name)) {
        if let Some(rest) = strip_cli_prefix(line, prefix)
            && let Some(tok) = rest.split_whitespace().next()
            && let Some(v) = version::parse(tok)
        {
            return Some(v);
        }
    }
    version::parse_package(line)
}

fn strip_cli_prefix<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    let (head, rest) = line.split_at_checked(prefix.len())?;
    if !head.eq_ignore_ascii_case(prefix) {
        return None;
    }
    match rest.chars().next() {
        None => Some(rest),
        Some(c) if c.is_ascii_whitespace() => Some(rest),
        _ => None,
    }
}

#[cfg(target_os = "macos")]
pub mod macos {
    use std::path::{Path, PathBuf};

    use super::{InstallSource, Installed, KeepApp, version};

    /// Where a `.app` might be: the system and the user's Applications folders.
    pub fn candidates(app: &KeepApp) -> Vec<PathBuf> {
        let bundle = format!("{}.app", app.name);
        let mut out = vec![Path::new("/Applications").join(&bundle)];
        if let Some(home) = dirs::home_dir() {
            out.push(home.join("Applications").join(&bundle));
        }
        out
    }

    pub fn detect(app: &KeepApp) -> Option<Installed> {
        candidates(app).iter().find_map(|p| read_bundle(app, p))
    }

    /// Reads a bundle's `Info.plist`; `None` unless it's really this app.
    pub fn read_bundle(app: &KeepApp, path: &Path) -> Option<Installed> {
        let plist = plist::Value::from_file(path.join("Contents/Info.plist")).ok()?;
        let dict = plist.as_dictionary()?;
        // Its current bundle id or an older one (TaxKeep was com.moneykeep.taxkeep).
        let id = dict.get("CFBundleIdentifier")?.as_string()?;
        if !app.bundle_ids().any(|b| b == id) {
            return None;
        }
        // `<App>Version` carries the full version including a pre-release suffix;
        // CFBundleShortVersionString only has the numbers.
        let full_key = format!("{}Version", app.name);
        let text =
            [full_key.as_str(), "CFBundleShortVersionString", "CFBundleVersion"].iter().find_map(|k| dict.get(k).and_then(plist::Value::as_string));
        Some(Installed {
            version: text.and_then(version::parse),
            source: InstallSource::MacApp,
            location: path.display().to_string(),
            package: None,
            externally_managed: false,
            origin: None,
            installation: None,
        })
    }
}

/// Newest version first; an entry with no version comes after every entry with one.
/// One setup program's uninstall entry (`HKLM\…\Uninstall\<App>`), as the registry holds it.
#[cfg(any(windows, test))]
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) struct SetupValues<'a> {
    pub display_name: &'a str,
    pub publisher: &'a str,
    pub display_version: &'a str,
    pub install_location: &'a str,
    pub quiet_uninstall: &'a str,
    pub uninstall: &'a str,
}

/// A setup program's install of an app, read from its uninstall entry.
#[cfg(any(windows, test))]
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SetupInstall {
    /// The entry's key name under `Uninstall` (the app's display name).
    pub key: String,
    pub version: Option<version::Version>,
    /// `InstallLocation`: the folder the setup put the app in.
    pub location: Option<PathBuf>,
    /// The silent uninstall command: program and arguments.
    pub uninstall: (String, String),
}

/// `app`'s setup install, when the entry `key` with `values` is one: the key is a name the app
/// has had, the display name and publisher are the app's (as for its MSI entries), and it says
/// how to uninstall it. A product code key is the Windows Installer's, never a setup's.
#[cfg(any(windows, test))]
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn setup_entry(app: &KeepApp, key: &str, values: &SetupValues<'_>) -> Option<SetupInstall> {
    if !app.names().any(|n| n.eq_ignore_ascii_case(key)) || !is_installer_entry(app, values.display_name, values.publisher) {
        return None;
    }
    let uninstall = crate::install::setup::uninstall_command(values.quiet_uninstall, values.uninstall)?;
    let location = values.install_location.trim().trim_matches('"');
    Some(SetupInstall {
        key: key.to_string(),
        version: version::parse(values.display_version.trim()),
        location: (!location.is_empty()).then(|| PathBuf::from(location)),
        uninstall,
    })
}

#[cfg(any(windows, test))]
fn newest_first(a: Option<&version::Version>, b: Option<&version::Version>) -> std::cmp::Ordering {
    match (b, a) {
        (Some(x), Some(y)) => version::compare(x, y),
        (Some(_), None) => std::cmp::Ordering::Greater,
        (None, Some(_)) => std::cmp::Ordering::Less,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

#[cfg(windows)]
pub mod windows {
    use std::path::PathBuf;

    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY};
    use winreg::{HKEY, RegKey};

    pub(crate) use super::SetupInstall;
    use super::{InstallSource, Installed, KeepApp, exe_candidates, install_dir_keys, is_installer_entry, suite_folders, version};

    const UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";

    /// Reads the ProductVersion string from a program's VERSIONINFO, in the first language that
    /// has one, without running the program or any other process.
    pub fn executable_version(path: &std::path::Path) -> Option<keep_launcher_core::version::Version> {
        let path = dunce::canonicalize(path).ok()?;
        let info = winsafe::HVERSIONINFO::GetFileVersionInfo(path.to_str()?).ok()?;
        let listed = info.langs_and_cps().map(<[_]>::to_vec).unwrap_or_default();
        let product = string_tables(&listed).into_iter().find_map(|table| info.str_val(table, "ProductVersion").ok())?;
        version::parse(product.trim())
    }

    /// The string tables to look in: the languages the file lists, then U.S. English in UTF-16,
    /// Windows-1252 and no code page, as .NET's `FileVersionInfo` also tries when a file lists none
    /// or its listed one has no strings.
    pub(crate) fn string_tables(listed: &[(winsafe::LANGID, winsafe::co::CP)]) -> Vec<(winsafe::LANGID, winsafe::co::CP)> {
        use winsafe::{LANGID, co};
        let us = LANGID::new(co::LANG::ENGLISH, co::SUBLANG::ENGLISH_US);
        listed.iter().copied().chain([(us, co::CP::UTF16), (us, co::CP::WINDOWS_1252), (us, co::CP::ACP)]).collect()
    }

    /// An MSI install found in the registry.
    pub struct MsiInstall {
        pub product_code: String,
        pub version: Option<keep_launcher_core::version::Version>,
        pub exe: Option<PathBuf>,
        /// The context of the uninstall record that named it: the machine's records are per-machine,
        /// the current account's own are per-user.
        pub scope: crate::install::MsiScope,
    }

    /// Where the uninstall records of an MSI are kept: the machine's (both registry views) and the account's own.
    const UNINSTALL_ROOTS: [(HKEY, u32); 3] = [(HKEY_LOCAL_MACHINE, KEY_WOW64_64KEY), (HKEY_LOCAL_MACHINE, KEY_WOW64_32KEY), (HKEY_CURRENT_USER, 0)];

    fn record_context(hive: HKEY) -> crate::install::MsiScope {
        if hive == HKEY_CURRENT_USER { crate::install::MsiScope::PerUser } else { crate::install::MsiScope::PerMachine }
    }

    /// The context every uninstall record of `product_code` places it in, when Windows Installer cannot be
    /// asked. All records are read, not the deduplicated list, so a copy registered both per-user and
    /// per-machine gives no answer and the update stops.
    pub fn record_scope(app: &KeepApp, product_code: &str) -> Option<crate::install::MsiScope> {
        let scopes = UNINSTALL_ROOTS.into_iter().filter_map(|(hive, flags)| {
            let root = RegKey::predef(hive).open_subkey_with_flags(UNINSTALL, KEY_READ | flags).ok()?;
            let key = root.open_subkey_with_flags(product_code, KEY_READ | flags).ok()?;
            let name: String = key.get_value("DisplayName").unwrap_or_default();
            let publisher: String = key.get_value("Publisher").unwrap_or_default();
            is_installer_entry(app, &name, &publisher).then(|| record_context(hive))
        });
        crate::msi_context::single_scope(scopes)
    }

    /// How an exe path is stored in `Installed::location`. One function builds it for `detect` and for
    /// the comparison below, so the two can never disagree on spelling.
    fn location_text(exe: &std::path::Path) -> String {
        exe.display().to_string()
    }

    /// Whether `location` is the exe the record `product_code` resolves in its own scope. False for
    /// a path that only the launch fallback (`exe_in_any_scope`) found, so an update never takes its
    /// folder from another scope.
    pub fn location_is_in_record_scope(app: &KeepApp, product_code: &str, location: &str) -> bool {
        msi_installs(app).into_iter().find(|m| m.product_code == product_code).and_then(|m| m.exe).is_some_and(|exe| location_text(&exe) == location)
    }

    /// Every Windows Installer entry for `app`, newest first.
    pub fn msi_installs(app: &KeepApp) -> Vec<MsiInstall> {
        let mut out = Vec::new();
        for (hive, flags) in UNINSTALL_ROOTS {
            let Ok(root) = RegKey::predef(hive).open_subkey_with_flags(UNINSTALL, KEY_READ | flags) else { continue };
            for code in root.enum_keys().flatten() {
                // App-authored uninstall rows are not MSI product identities.
                if !crate::msi_context::valid_product_code(&code) {
                    continue;
                }
                let Ok(key) = root.open_subkey_with_flags(&code, KEY_READ | flags) else { continue };
                let name: String = key.get_value("DisplayName").unwrap_or_default();
                let publisher: String = key.get_value("Publisher").unwrap_or_default();
                if !is_installer_entry(app, &name, &publisher) || out.iter().any(|m: &MsiInstall| m.product_code == code) {
                    continue;
                }
                let version = key.get_value::<String, _>("DisplayVersion").ok().and_then(|v| version::parse(&v));
                let location: String = key.get_value("InstallLocation").unwrap_or_default();
                let exe =
                    exe_path(app, (!location.is_empty()).then(|| PathBuf::from(location)).or_else(|| install_dir(app, hive, flags)), hive, flags);
                out.push(MsiInstall { product_code: code, version, exe, scope: record_context(hive) });
            }
        }
        out.sort_by(|a, b| super::newest_first(a.version.as_ref(), b.version.as_ref()));
        out
    }

    /// The folder the app's MSI remembers for its next upgrade, read from the same registry view as the
    /// uninstall record that named the install. A per-machine record (HKLM) therefore reads HKLM, and a
    /// per-user record (HKCU, dual-scope packages) reads HKCU, so one scope never resolves to the other's folder.
    fn install_dir(app: &KeepApp, hive: HKEY, flags: u32) -> Option<PathBuf> {
        install_dir_keys(app).into_iter().find_map(|path| {
            let key = RegKey::predef(hive).open_subkey_with_flags(path, KEY_READ | flags).ok()?;
            let dir = key.get_value::<String, _>("InstallDir").ok()?;
            (!dir.is_empty()).then(|| PathBuf::from(dir))
        })
    }

    /// The app's exe for one uninstall record, found only in that record's own scope: the recorded
    /// folder first, then `App Paths` in the record's hive and registry view, then the usual folders
    /// for that scope (`MoneyKeep` suite folder first, then the bare root, for older installs). A
    /// per-machine entry therefore never stands in for a per-user record, or the other way round.
    fn exe_path(app: &KeepApp, location: Option<PathBuf>, hive: HKEY, flags: u32) -> Option<PathBuf> {
        if let Some(found) = exe_candidates(app, location.as_deref(), &[]).into_iter().find(|p| p.exists()) {
            return Some(found);
        }
        for stem in app.exe_names() {
            let app_paths = format!(r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\{stem}.exe");
            if let Ok(key) = RegKey::predef(hive).open_subkey_with_flags(&app_paths, KEY_READ | flags)
                && let Ok(path) = key.get_value::<String, _>("")
            {
                let p = PathBuf::from(path.trim_matches('"'));
                if p.exists() {
                    return Some(p);
                }
            }
        }
        let roots: Vec<PathBuf> = if hive == HKEY_CURRENT_USER {
            // Per-user installs (dual-scope packages) live in the user's own Programs folder.
            std::env::var_os("LOCALAPPDATA").map(|local| PathBuf::from(local).join("Programs")).into_iter().collect()
        } else {
            let mut roots: Vec<PathBuf> = Vec::new();
            for var in ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"] {
                if let Some(pf) = std::env::var_os(var).map(PathBuf::from)
                    && !roots.contains(&pf)
                {
                    roots.push(pf);
                }
            }
            roots
        };
        exe_in_roots(app, roots)
    }

    /// Where the exe may be under the given install roots, best first: the launcher's own folder, then
    /// the `MoneyKeep` suite folder of each root, then the bare roots (older installs). Not checked on
    /// disk.
    fn candidates_in_roots(app: &KeepApp, roots: &[PathBuf]) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = suite_folders(app, roots).iter().flat_map(|dir| exe_candidates(app, Some(dir), &[])).collect();
        let mut dirs: Vec<PathBuf> = roots.iter().map(|root| root.join("MoneyKeep")).collect();
        dirs.extend(roots.iter().cloned());
        out.extend(exe_candidates(app, None, &dirs));
        out
    }

    /// The first exe under the given install roots.
    pub(super) fn exe_in_roots(app: &KeepApp, roots: Vec<PathBuf>) -> Option<PathBuf> {
        candidates_in_roots(app, &roots).into_iter().find(|p| p.exists())
    }

    /// Every exe under the given install roots, in the order `exe_in_roots` prefers them.
    pub(super) fn exe_all_in_roots(app: &KeepApp, roots: Vec<PathBuf>) -> Vec<PathBuf> {
        candidates_in_roots(app, &roots).into_iter().filter(|p| p.exists()).collect()
    }

    /// The app's exe directly in `folder`, under any name it has shipped as.
    pub(super) fn exe_in_folder(app: &KeepApp, folder: &std::path::Path) -> Option<PathBuf> {
        exe_candidates(app, Some(folder), &[]).into_iter().find(|p| p.exists())
    }

    /// Last resort for opening an app whose uninstall record names no folder and whose exe is not in
    /// the record's own scope. The package records its folder (`InstallDir`) in the hive of the account
    /// that installed it, which need not be the hive of its uninstall record: a per-user install can
    /// leave a per-machine record. So the folder any hive or view recorded is tried first (this finds
    /// a custom folder), then the usual folders of every scope. This only decides which exe "Open"
    /// runs and where "Show in folder" goes. An update never takes its folder from here
    /// (`location_is_in_record_scope`), and the record's scope is unchanged. Every candidate is
    /// collected and the one whose own version is the record's is chosen, so a leftover older copy in
    /// another scope is not opened under the newer record's version (`pick_candidate`).
    fn exe_in_any_scope(app: &KeepApp, version: Option<&version::Version>) -> Option<PathBuf> {
        let mut found: Vec<PathBuf> =
            UNINSTALL_ROOTS.into_iter().filter_map(|(hive, flags)| exe_in_folder(app, &install_dir(app, hive, flags)?)).collect();
        let mut roots: Vec<PathBuf> = std::env::var_os("LOCALAPPDATA").map(|local| PathBuf::from(local).join("Programs")).into_iter().collect();
        for var in ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(pf) = std::env::var_os(var).map(PathBuf::from)
                && !roots.contains(&pf)
            {
                roots.push(pf);
            }
        }
        found.extend(exe_all_in_roots(app, roots));
        pick_candidate(found, version, executable_version)
    }

    /// The first candidate whose version is `wanted`; with no match (or no version to match, or exes
    /// without version info) the first candidate, in the order they were found.
    pub(super) fn pick_candidate(
        candidates: Vec<PathBuf>,
        wanted: Option<&version::Version>,
        version_of: impl Fn(&std::path::Path) -> Option<version::Version>,
    ) -> Option<PathBuf> {
        let matching = wanted.and_then(|want| candidates.iter().find(|c| version_of(c).as_ref() == Some(want)));
        matching.or_else(|| candidates.first()).cloned()
    }

    /// Every setup program install of `app`, newest first: the machine's uninstall entries (both
    /// registry views) named after the app. Setups install per machine only, so the account's own
    /// entries, which it can write without permission, are never run elevated.
    pub(crate) fn setup_installs(app: &KeepApp) -> Vec<SetupInstall> {
        let mut out: Vec<SetupInstall> = Vec::new();
        for flags in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            let Ok(root) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(UNINSTALL, KEY_READ | flags) else { continue };
            for name in app.names() {
                let Ok(key) = root.open_subkey_with_flags(name, KEY_READ | flags) else { continue };
                let text = |value: &str| key.get_value::<String, _>(value).unwrap_or_default();
                let (display_name, publisher, display_version) = (text("DisplayName"), text("Publisher"), text("DisplayVersion"));
                let (install_location, quiet_uninstall, uninstall) = (text("InstallLocation"), text("QuietUninstallString"), text("UninstallString"));
                let values = super::SetupValues {
                    display_name: &display_name,
                    publisher: &publisher,
                    display_version: &display_version,
                    install_location: &install_location,
                    quiet_uninstall: &quiet_uninstall,
                    uninstall: &uninstall,
                };
                if let Some(found) = super::setup_entry(app, name, &values)
                    && !out.iter().any(|seen| seen.key.eq_ignore_ascii_case(&found.key) && seen.location == found.location)
                {
                    out.push(found);
                }
            }
        }
        out.sort_by(|a, b| super::newest_first(a.version.as_ref(), b.version.as_ref()));
        out
    }

    /// The setup install as an [`Installed`]: its exe in `InstallLocation` (under any name the app
    /// has shipped as), else the app's name, as for an MSI whose exe isn't found.
    fn setup_installed(app: &KeepApp, s: SetupInstall) -> Installed {
        let exe = s.location.as_deref().and_then(|folder| exe_in_folder(app, folder));
        Installed {
            version: s.version,
            source: InstallSource::Setup,
            location: exe.map_or_else(|| app.name.to_string(), |p| location_text(&p)),
            package: Some(s.key),
            externally_managed: false,
            origin: None,
            installation: None,
        }
    }

    pub fn detect(app: &KeepApp) -> Option<Installed> {
        let setup = setup_installs(app).into_iter().next();
        let Some(m) = msi_installs(app).into_iter().next() else { return setup.map(|s| setup_installed(app, s)) };
        // Both kinds registered: the newer one is the copy in use (an MSI on a tie).
        if let Some(s) = setup
            && super::newest_first(m.version.as_ref(), s.version.as_ref()) == std::cmp::Ordering::Greater
        {
            return Some(setup_installed(app, s));
        }
        let location = m.exe.or_else(|| exe_in_any_scope(app, m.version.as_ref())).map_or_else(|| app.name.to_string(), |p| location_text(&p));
        Some(Installed {
            version: m.version,
            source: InstallSource::Msi,
            location,
            package: Some(m.product_code),
            externally_managed: false,
            origin: None,
            installation: None,
        })
    }
}

#[cfg(target_os = "linux")]
pub mod linux {
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use super::{InstallSource, Installed, KeepApp, Snapshot, appimage_version, parse_cli_version, version};
    use crate::packages::{self, Package};
    use crate::process::capture;

    /// Installed MoneyKeep App packages from every package manager on this system (dpkg, rpm,
    /// pacman, Flatpak). Read once per detection pass.
    pub fn packages() -> Vec<Package> {
        let ours = |name: &str| {
            keep_launcher_core::SUITE.iter().any(|a| {
                [InstallSource::Deb, InstallSource::Rpm, InstallSource::Pacman, InstallSource::Flatpak]
                    .iter()
                    .any(|m| crate::packages::candidate_names(a, *m).iter().any(|n| n == name))
            })
        };
        let t = Duration::from_secs(10);
        let mut out = Vec::new();
        let mut add = |manager, user, list: Vec<(String, String)>| {
            out.extend(list.into_iter().filter(|(n, _)| ours(n)).map(|(name, version)| Package {
                manager,
                name,
                version,
                user,
                origin: None,
                installation: None,
            }));
        };
        if let Some(list) = capture("dpkg-query", &["-W", "-f", "${Package}|${db:Status-Status}|${Version}\n"], t) {
            add(InstallSource::Deb, false, packages::parse_dpkg(&list));
        }
        if let Some(list) = capture("rpm", &["-qa", "--qf", "%{NAME}|%{VERSION}\n"], t) {
            add(InstallSource::Rpm, false, packages::parse_rpm(&list));
        }
        // Only run `pacman` where its database exists: elsewhere `pacman` on PATH may be an unrelated program
        // (the Pac-Man game in /usr/games on Debian-family systems).
        if has_pacman_db()
            && let Some(list) = capture("pacman", &["-Q"], t)
        {
            add(InstallSource::Pacman, false, packages::parse_pacman(&list));
        }
        let arch = format!("--arch={}", std::env::consts::ARCH);
        // User installs are marked so `packages::find` reads the user copy the launcher updates, not a
        // stale system one (#881). The unscoped list after it still covers every system installation,
        // named ones included (`--system` is only the default one).
        let mut user_ids = Vec::new();
        for scope in [&["--user"][..], &[]] {
            let list_args = [&["list", "--app"][..], scope, &[&arch, "--columns=application,version,origin,installation"]].concat();
            if let Some(list) = crate::process::capture_flatpak(&list_args, t) {
                let mut flatpaks = packages::parse_flatpak_list_with_origin(&list);
                // The unscoped list repeats the user installs: the user pass already has them.
                flatpaks.retain(|(id, _, _, _)| !user_ids.contains(id));
                if !scope.is_empty() {
                    user_ids.extend(flatpaks.iter().map(|(id, _, _, _)| id.clone()));
                }
                // Bundles built from a release tarball often list no version: ask Flatpak directly.
                for (id, version, _, installation) in flatpaks
                    .iter_mut()
                    .filter(|(id, v, _, _)| v.is_empty() && keep_launcher_core::SUITE.iter().any(|a| a.bundle_ids().any(|b| b == *id)))
                {
                    let scope_arg = packages::flatpak_scope(if scope.is_empty() { installation.as_deref() } else { Some("user") });
                    let info_args = ["info", &scope_arg, &arch, id];
                    if let Some(v) = crate::process::capture_flatpak(&info_args, t).as_deref().and_then(packages::parse_flatpak_info) {
                        *version = v;
                    }
                }
                let is_user = !scope.is_empty();
                out.extend(flatpaks.into_iter().filter(|(n, _, _, _)| ours(n)).map(|(name, version, origin, installation)| {
                    let user_install = is_user || installation.as_deref() == Some("user");
                    let resolved_installation = installation.or_else(|| if user_install { Some("user".into()) } else { Some("system".into()) });
                    Package { manager: InstallSource::Flatpak, name, version, user: user_install, origin, installation: resolved_installation }
                }));
            }
        }
        out
    }

    /// The local package database of Arch's pacman.
    pub(crate) const PACMAN_DB: &str = "/var/lib/pacman/local";

    /// True when `db` is a directory, i.e. this system really uses pacman.
    pub(crate) fn pacman_db_present(db: &Path) -> bool {
        db.is_dir()
    }

    /// True when the system's default pacman database directory exists.
    pub(crate) fn has_pacman_db() -> bool {
        pacman_db_present(Path::new(PACMAN_DB))
    }

    /// Where a package put the app's binary: on `PATH`, else `/usr/bin/<id>`.
    fn binary_on_path(id: &str) -> String {
        crate::process::which(id).unwrap_or_else(|| PathBuf::from("/usr/bin").join(id)).display().to_string()
    }

    /// `~/Applications`, where the launcher puts AppImages (the AppImageLauncher convention).
    pub fn appimage_dir() -> Option<PathBuf> {
        dirs::home_dir().map(|h| h.join("Applications"))
    }

    /// Every folder searched for AppImages, the launcher's own first: `~/Applications`,
    /// `~/AppImages` (Gear Lever's default) and `~/.local/bin`.
    pub fn appimage_dirs() -> Vec<PathBuf> {
        let Some(home) = dirs::home_dir() else { return Vec::new() };
        vec![home.join("Applications"), home.join("AppImages"), home.join(".local").join("bin")]
    }

    /// AppImages of `app` in `dirs`, newest first (unknown versions last; earlier folders win
    /// ties).
    pub fn appimages_in(app: &KeepApp, dirs: &[PathBuf]) -> Vec<(PathBuf, Option<version::Version>)> {
        let stems: Vec<&str> = app.stems().collect();
        let mut out: Vec<(PathBuf, Option<version::Version>)> = Vec::new();
        for dir in dirs {
            let Ok(entries) = std::fs::read_dir(dir) else { continue };
            let mut found: Vec<(PathBuf, Option<version::Version>)> = entries
                .flatten()
                .filter_map(|e| {
                    let name = e.file_name().into_string().ok()?;
                    let version = appimage_version(&name, &stems)?;
                    let path = e.path();
                    path.is_file().then(|| (path, version.as_deref().and_then(version::parse)))
                })
                .collect();
            found.sort_by(|a, b| a.0.cmp(&b.0));
            out.extend(found);
        }
        // Stable sort: equal versions keep the folder order above.
        out.sort_by(|a, b| match (&b.1, &a.1) {
            (Some(x), Some(y)) => version::compare(x, y),
            (Some(_), None) => std::cmp::Ordering::Greater,
            (None, Some(_)) => std::cmp::Ordering::Less,
            (None, None) => std::cmp::Ordering::Equal,
        });
        out
    }

    /// AppImages of `app` in any of the usual folders, newest first.
    pub fn appimages(app: &KeepApp) -> Vec<(PathBuf, Option<version::Version>)> {
        appimages_in(app, &appimage_dirs())
    }

    /// How the app is installed, in this order: an AppImage the launcher put down, a package
    /// (deb, rpm, pacman or Flatpak), else a binary found on disk.
    pub fn detect(app: &KeepApp, snapshot: &Snapshot) -> Option<Installed> {
        if let Some((path, version)) = appimages(app).into_iter().next() {
            return Some(Installed {
                version,
                source: InstallSource::AppImage,
                location: path.display().to_string(),
                package: None,
                externally_managed: false,
                origin: None,
                installation: None,
            });
        }
        let order = [InstallSource::Deb, InstallSource::Rpm, InstallSource::Pacman, InstallSource::Flatpak];
        if let Some(p) = packages::find(app, &snapshot.packages, &order) {
            // A Flatpak runs under the id it was found under, which may be an older one.
            let location = if p.manager == InstallSource::Flatpak { p.name.clone() } else { binary_on_path(app.id) };
            let externally_managed = p.manager == InstallSource::Flatpak
                && p.origin.as_deref().is_none_or(|origin| {
                    if packages::is_flatpark_origin(origin) {
                        return true;
                    }
                    let scope = packages::flatpak_scope(p.installation.as_deref());
                    crate::process::capture_flatpak(&["remotes", &scope, "--show-disabled", "--columns=name,url"], Duration::from_secs(10))
                        .is_none_or(|remotes| packages::remote_manages_flatpak(origin, &remotes))
                });
            return Some(Installed {
                version: packages::version_of(p),
                source: p.manager,
                location,
                package: Some(p.name.clone()),
                externally_managed,
                origin: p.origin.clone(),
                installation: p.installation.clone(),
            });
        }
        detect_system_install(app)
    }

    /// When the package managers don't report the app (a query timed out or was locked, an
    /// unlisted package name, or a sandbox that can't see them) but its binary is there. It is
    /// reported as a system binary: the launcher can open it but leaves updating and removing it
    /// to whatever installed it, since it can't tell which package manager that was.
    fn detect_system_install(app: &KeepApp) -> Option<Installed> {
        let bin = find_binary(app.id)?;
        Some(Installed {
            version: binary_version(&bin, app),
            source: InstallSource::Binary,
            location: bin.display().to_string(),
            package: None,
            externally_managed: false,
            origin: None,
            installation: None,
        })
    }

    /// The app's binary on `PATH`, else in `/usr/bin`, `/usr/local/bin` or `~/.local/bin`.
    fn find_binary(id: &str) -> Option<PathBuf> {
        let on_path = crate::process::which(id);
        let usual = ["/usr/bin", "/usr/local/bin"].into_iter().map(PathBuf::from).chain(dirs::home_dir().map(|h| h.join(".local/bin")));
        on_path.or_else(|| usual.map(|d| d.join(id)).find(|p| p.is_file()))
    }

    /// The version `bin --version` prints, or `None` when it can't be started, doesn't finish
    /// within 5 seconds (it is then killed), exits with an error, or prints no version.
    pub(crate) fn binary_version(bin: &Path, app: &KeepApp) -> Option<version::Version> {
        let out = crate::process::capture(bin.to_str()?, &["--version"], Duration::from_secs(5))?;
        parse_cli_version(out.trim(), app)
    }
}

#[cfg(target_os = "freebsd")]
pub mod freebsd {
    use std::path::Path;
    use std::time::Duration;

    use super::{InstallSource, Installed, KeepApp, parse_cli_version, version};

    pub fn detect(app: &KeepApp) -> Option<Installed> {
        let bin = Path::new("/usr/local/bin").join(app.id);
        if !bin.exists() {
            return None;
        }
        Some(Installed {
            version: binary_version(&bin, app),
            source: InstallSource::Binary,
            location: bin.display().to_string(),
            package: None,
            externally_managed: false,
            origin: None,
            installation: None,
        })
    }

    /// `<app> --version` prints `<app> 0.5.0 (sha, date)`.
    fn binary_version(bin: &Path, app: &KeepApp) -> Option<version::Version> {
        let out = crate::process::capture(bin.to_str()?, &["--version"], Duration::from_secs(5))?;
        parse_cli_version(out.trim(), app)
    }
}

#[cfg(all(test, target_os = "linux"))]
mod linux_tests {
    use super::*;

    #[test]
    fn ours_filter_matches_rpm_packages() {
        let ours = |name: &str| {
            keep_launcher_core::SUITE.iter().any(|a| {
                [InstallSource::Deb, InstallSource::Rpm, InstallSource::Pacman, InstallSource::Flatpak]
                    .iter()
                    .any(|m| crate::packages::candidate_names(a, *m).iter().any(|n| n == name))
            })
        };
        assert!(ours("moneykeep"));
        assert!(ours("com.moneykeep.moneykeep"));
        assert!(ours("calckeep"));
        assert!(ours("com.moneykeep.calckeep"));
        assert!(!ours("unrelated-package"));
    }

    #[test]
    fn pacman_is_only_queried_where_its_database_exists() {
        let root = tempfile::tempdir().unwrap();
        assert!(!linux::pacman_db_present(&root.path().join("local")));
        std::fs::create_dir(root.path().join("local")).unwrap();
        assert!(linux::pacman_db_present(&root.path().join("local")));
    }

    /// `linux::binary_version` on a script the test just wrote. Another test thread that forks
    /// while the script is still open for writing holds that descriptor until it execs, and running
    /// the script in that window fails with ETXTBSY ("Text file busy"), so a miss is retried.
    fn binary_version_of_new_script(bin: &Path, app: &KeepApp) -> Option<version::Version> {
        for _ in 0..20 {
            if let Some(v) = linux::binary_version(bin, app) {
                return Some(v);
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        None
    }

    #[test]
    fn reads_binary_version_from_cli() {
        let photo = KeepApp::by_id("moneykeep").unwrap();
        let temp_dir = tempfile::tempdir().unwrap();
        let bin = temp_dir.path().join("moneykeep");
        std::fs::write(&bin, "#!/bin/sh\necho 'moneykeep 0.6.0 (0c72d9542, 2026-10-10)'\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let ver = binary_version_of_new_script(&bin, photo);
        assert_eq!(ver.unwrap().to_string(), "0.6.0");
    }

    #[test]
    fn reads_binary_version_when_the_cli_uses_the_display_name_or_a_former_id() {
        let temp_dir = tempfile::tempdir().unwrap();
        use std::os::unix::fs::PermissionsExt;
        let write = |name: &str, script: &str| {
            let bin = temp_dir.path().join(name);
            std::fs::write(&bin, script).unwrap();
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
            bin
        };
        let sound = write("calckeep", "#!/bin/sh\necho 'CalcKeep 0.5.0'\n");
        assert_eq!(binary_version_of_new_script(&sound, KeepApp::by_id("calckeep").unwrap()).unwrap().to_string(), "0.5.0");
        let pdf = write("ledgerkeep", "#!/bin/sh\necho 'bookkeep 0.2.1'\n");
        assert_eq!(binary_version_of_new_script(&pdf, &keep_launcher_core::fixtures::RENAMED).unwrap().to_string(), "0.2.1");
    }
}

#[cfg(test)]
mod tests {
    use keep_launcher_core::SUITE;

    use super::*;

    #[test]
    fn msi_entries_sort_newest_first_and_unversioned_last() {
        use std::cmp::Ordering::{Equal, Greater, Less};
        let (old, new) = (version::parse("0.4.0"), version::parse("0.5.0"));
        // Each arm by hand: the sort below never asks about None against Some in that order.
        assert_eq!(newest_first(new.as_ref(), old.as_ref()), Less);
        assert_eq!(newest_first(old.as_ref(), new.as_ref()), Greater);
        assert_eq!(newest_first(new.as_ref(), None), Less);
        assert_eq!(newest_first(None, new.as_ref()), Greater);
        assert_eq!(newest_first(None, None), Equal);
        let mut versions = vec![None, old.clone(), new.clone(), None];
        versions.sort_by(|a, b| newest_first(a.as_ref(), b.as_ref()));
        assert_eq!(versions, vec![new, old, None, None]);
    }

    /// A file without VERSIONINFO, or one that isn't there, has no version.
    #[cfg(windows)]
    #[test]
    fn a_file_without_version_info_has_no_version() {
        let dir = tempfile::tempdir().unwrap();
        let text = dir.path().join("moneykeep.exe");
        std::fs::write(&text, "not a program").unwrap();
        assert_eq!(windows::executable_version(&text), None);
        assert_eq!(windows::executable_version(&dir.path().join("missing.exe")), None);
    }

    /// Without a listed language, the version is still looked for in the string tables .NET's
    /// `FileVersionInfo` falls back to (040904B0, 040904E4, 04090000), after any listed one.
    #[cfg(windows)]
    #[test]
    fn the_version_is_looked_for_in_the_usual_string_tables_too() {
        use winsafe::{LANGID, co};
        let key = |(lang, cp): (LANGID, co::CP)| format!("{:04x}{:04x}", u16::from(lang), u16::from(cp));
        let keys = |listed: &[(LANGID, co::CP)]| windows::string_tables(listed).into_iter().map(key).collect::<Vec<_>>();
        assert_eq!(keys(&[]), ["040904b0", "040904e4", "04090000"]);
        let german = (LANGID::new(co::LANG::GERMAN, co::SUBLANG::GERMAN), co::CP::UTF16);
        assert_eq!(keys(&[german]), ["040704b0", "040904b0", "040904e4", "04090000"]);
    }

    /// A program whose VERSIONINFO has a ProductVersion in the U.S. English UTF-16 table but no
    /// `\VarFileInfo\Translation` entry still has a version.
    #[cfg(windows)]
    #[test]
    fn a_version_without_a_translation_entry_is_found() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("moneykeep.exe");
        std::fs::write(&exe, resource_only_program("1.2.3")).unwrap();
        assert_eq!(windows::executable_version(&exe), version::parse("1.2.3"));
    }

    /// A VERSIONINFO block node: length, value length, type, the key, then the value and the
    /// children, each aligned to 4 bytes.
    #[cfg(windows)]
    fn version_node(key: &str, value: &[u8], value_length: u16, text: bool, children: &[Vec<u8>]) -> Vec<u8> {
        let align = |bytes: &mut Vec<u8>| bytes.resize(bytes.len().next_multiple_of(4), 0);
        let mut node = vec![0, 0];
        node.extend(value_length.to_le_bytes());
        node.extend(u16::from(text).to_le_bytes());
        node.extend(key.encode_utf16().chain([0]).flat_map(u16::to_le_bytes));
        align(&mut node);
        node.extend(value);
        for child in children {
            align(&mut node);
            node.extend(child);
        }
        let length = u16::try_from(node.len()).unwrap();
        node[..2].copy_from_slice(&length.to_le_bytes());
        node
    }

    /// A 64-bit program file holding only a version resource (`ProductVersion` = `product`, in
    /// the 040904B0 string table) and no `\VarFileInfo\Translation` entry.
    #[cfg(windows)]
    fn resource_only_program(product: &str) -> Vec<u8> {
        let put = |bytes: &mut Vec<u8>, at: usize, value: &[u8]| bytes[at..at + value.len()].copy_from_slice(value);
        let text: Vec<u8> = product.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
        let string = version_node("ProductVersion", &text, u16::try_from(product.len() + 1).unwrap(), true, &[]);
        let table = version_node("040904b0", &[], 0, true, &[string]);
        let strings = version_node("StringFileInfo", &[], 0, true, &[table]);
        let mut fixed = vec![0u8; 52];
        put(&mut fixed, 0, &0xFEEF_04BDu32.to_le_bytes());
        put(&mut fixed, 4, &0x0001_0000u32.to_le_bytes());
        let info = version_node("VS_VERSION_INFO", &fixed, 52, false, &[strings]);

        // The resource section: type 16 (RT_VERSION) > id 1 > language 0x0409 > the data.
        const RVA: u32 = 0x1000;
        let mut rsrc = vec![0u8; 0x58];
        for (dir, id, next) in [(0x00, 16u32, 0x8000_0018u32), (0x18, 1, 0x8000_0030), (0x30, 0x0409, 0x48)] {
            put(&mut rsrc, dir + 14, &1u16.to_le_bytes());
            put(&mut rsrc, dir + 16, &id.to_le_bytes());
            put(&mut rsrc, dir + 20, &next.to_le_bytes());
        }
        put(&mut rsrc, 0x48, &(RVA + 0x58).to_le_bytes());
        put(&mut rsrc, 0x4C, &u32::try_from(info.len()).unwrap().to_le_bytes());
        rsrc.extend(&info);
        let raw = u32::try_from(rsrc.len().next_multiple_of(0x200)).unwrap();
        let rsrc_len = u32::try_from(rsrc.len()).unwrap();

        let mut file = vec![0u8; 0x200];
        put(&mut file, 0, b"MZ");
        put(&mut file, 0x3C, &0x40u32.to_le_bytes());
        put(&mut file, 0x40, b"PE\0\0");
        // COFF header: x64, one section, a 240-byte optional header, an executable DLL image.
        put(&mut file, 0x44, &0x8664u16.to_le_bytes());
        put(&mut file, 0x46, &1u16.to_le_bytes());
        put(&mut file, 0x54, &240u16.to_le_bytes());
        put(&mut file, 0x56, &0x2022u16.to_le_bytes());
        // PE32+ optional header.
        let opt = 0x58;
        put(&mut file, opt, &0x20Bu16.to_le_bytes());
        put(&mut file, opt + 8, &raw.to_le_bytes());
        put(&mut file, opt + 24, &0x1_4000_0000u64.to_le_bytes());
        put(&mut file, opt + 32, &0x1000u32.to_le_bytes());
        put(&mut file, opt + 36, &0x200u32.to_le_bytes());
        put(&mut file, opt + 40, &6u16.to_le_bytes());
        put(&mut file, opt + 48, &6u16.to_le_bytes());
        put(&mut file, opt + 56, &(RVA + rsrc_len.next_multiple_of(0x1000)).to_le_bytes());
        put(&mut file, opt + 60, &0x200u32.to_le_bytes());
        put(&mut file, opt + 68, &2u16.to_le_bytes());
        put(&mut file, opt + 72, &0x10_0000u64.to_le_bytes());
        put(&mut file, opt + 80, &0x1000u64.to_le_bytes());
        put(&mut file, opt + 88, &0x10_0000u64.to_le_bytes());
        put(&mut file, opt + 96, &0x1000u64.to_le_bytes());
        put(&mut file, opt + 108, &16u32.to_le_bytes());
        put(&mut file, opt + 112 + 2 * 8, &RVA.to_le_bytes());
        put(&mut file, opt + 112 + 2 * 8 + 4, &rsrc_len.to_le_bytes());
        // The .rsrc section header.
        let section = opt + 240;
        put(&mut file, section, b".rsrc\0\0\0");
        put(&mut file, section + 8, &rsrc_len.to_le_bytes());
        put(&mut file, section + 12, &RVA.to_le_bytes());
        put(&mut file, section + 16, &raw.to_le_bytes());
        put(&mut file, section + 20, &0x200u32.to_le_bytes());
        put(&mut file, section + 36, &0x4000_0040u32.to_le_bytes());
        rsrc.resize(usize::try_from(raw).unwrap(), 0);
        file.extend(rsrc);
        file
    }

    fn setup_values<'a>(name: &'a str, publisher: &'a str) -> SetupValues<'a> {
        SetupValues {
            display_name: name,
            publisher,
            display_version: "0.1.0-alpha.1",
            install_location: r"C:\Program Files\MoneyKeep",
            quiet_uninstall: r#""C:\Program Files\MoneyKeep\uninstall.exe" /S"#,
            uninstall: r#""C:\Program Files\MoneyKeep\uninstall.exe""#,
        }
    }

    #[test]
    fn a_setup_install_is_found_by_its_uninstall_entry() {
        let app = KeepApp::by_id("moneykeep").unwrap();
        let found = setup_entry(app, "MoneyKeep", &setup_values("MoneyKeep", "MoneyKeep Project")).unwrap();
        assert_eq!(found.key, "MoneyKeep");
        assert_eq!(found.version, version::parse("0.1.0-alpha.1"));
        assert_eq!(found.location, Some(PathBuf::from(r"C:\Program Files\MoneyKeep")));
        assert_eq!(found.uninstall, (r"C:\Program Files\MoneyKeep\uninstall.exe".to_string(), "/S".to_string()));
        // Registry key names are not case-sensitive; a quoted location is unquoted.
        let quoted = SetupValues { install_location: r#""C:\Program Files\MoneyKeep""#, ..setup_values("MoneyKeep", "MoneyKeep Project") };
        assert_eq!(setup_entry(app, "moneykeep", &quoted).unwrap().location, Some(PathBuf::from(r"C:\Program Files\MoneyKeep")));
        // An install from before the publisher rename is still ours.
        assert!(setup_entry(app, "MoneyKeep", &setup_values("MoneyKeep", "MoneyKeep Project Inc")).is_some());
        // Without a location or a readable version it is still found; the version is then unknown.
        let bare = SetupValues { install_location: "", display_version: "", ..setup_values("MoneyKeep", "MoneyKeep Project") };
        let found = setup_entry(app, "MoneyKeep", &bare).unwrap();
        assert_eq!((found.location, found.version), (None, None));
    }

    #[test]
    fn other_uninstall_entries_are_not_setup_installs() {
        let app = KeepApp::by_id("calckeep").unwrap();
        let ok = setup_values("CalcKeep", "MoneyKeep Project");
        assert!(setup_entry(app, "CalcKeep", &ok).is_some());
        // Someone else's program of the same name, another app's entry, a product code key.
        assert!(setup_entry(app, "CalcKeep", &setup_values("CalcKeep", "Example Corp")).is_none());
        assert!(setup_entry(app, "CalcKeep", &setup_values("MoneyKeep", "MoneyKeep Project")).is_none());
        assert!(setup_entry(app, "{8F1C2D3E-1111-2222-3333-444455556666}", &ok).is_none());
        assert!(setup_entry(app, "MoneyKeep", &ok).is_none());
        // An entry that doesn't say how to remove it is not one the launcher can manage.
        assert!(setup_entry(app, "CalcKeep", &SetupValues { quiet_uninstall: "", uninstall: "", ..ok }).is_none());
        // Without a quiet string, the normal one is run silently.
        let normal = SetupValues { quiet_uninstall: "", ..setup_values("CalcKeep", "MoneyKeep Project") };
        assert_eq!(setup_entry(app, "CalcKeep", &normal).unwrap().uninstall.1, "/S");
    }

    #[test]
    fn recognises_appimage_names_conservatively() {
        let pdf = ["ledgerkeep", "bookkeep"];
        let v = |n: &str| appimage_version(n, &pdf);
        assert_eq!(v("ledgerkeep-0.3.0-linux-x86_64.AppImage"), Some(Some("0.3.0".into())));
        assert_eq!(v("bookkeep-0.2.1-linux-aarch64.appimage"), Some(Some("0.2.1".into())));
        assert_eq!(v("ledgerkeep-0.3.0-rc.1-linux-x86_64.AppImage"), Some(Some("0.3.0-rc.1".into())));
        // Gear Lever's rename, and a plain rename by hand.
        assert_eq!(v("ledgerkeep.appimage"), Some(None));
        assert_eq!(v("LedgerKeep.AppImage"), Some(None));
        for other in [
            "ledgerkeep-0.3.0-linux-x86_64.AppImage.zsync",
            "ledgerkeep-0.3.0-linux-x86_64.deb",
            "ledgerkeep-0.3.0-windows-x64.AppImage",
            "ledgerkeep-cli-0.3.0-linux-x86_64.AppImage",
            "moneykeep-0.6.0-linux-x86_64.AppImage",
            "myledgerkeep.AppImage",
            "ledgerkeep-latest.AppImage",
            ".appimage",
            "x",
        ] {
            assert_eq!(v(other), None, "{other}");
        }
    }

    #[test]
    fn parse_cli_version_accepts_id_display_name_and_former_id() {
        let photo = KeepApp::by_id("moneykeep").unwrap();
        let sound = KeepApp::by_id("calckeep").unwrap();
        let pdf = &keep_launcher_core::fixtures::RENAMED;
        assert_eq!(parse_cli_version("moneykeep 0.7.0 (dev build)", photo).unwrap().to_string(), "0.7.0");
        assert_eq!(parse_cli_version("CalcKeep 0.5.0", sound).unwrap().to_string(), "0.5.0");
        assert_eq!(parse_cli_version("bookkeep 0.2.1", pdf).unwrap().to_string(), "0.2.1");
        assert_eq!(parse_cli_version("LedgerKeep 0.6.0", pdf).unwrap().to_string(), "0.6.0");
        assert!(strip_cli_prefix("moneykeep-cli 1.0.0", "moneykeep").is_none());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn finds_appimages_in_every_folder_newest_first() {
        let app = KeepApp::by_id("moneykeep").unwrap();
        let root = tempfile::tempdir().unwrap();
        let (apps, gear, bin) = (root.path().join("Applications"), root.path().join("AppImages"), root.path().join("bin"));
        for d in [&apps, &gear, &bin] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(apps.join("moneykeep-0.5.0-linux-x86_64.AppImage"), b"").unwrap();
        std::fs::write(apps.join("calckeep-0.3.0-linux-x86_64.AppImage"), b"").unwrap();
        std::fs::write(gear.join("moneykeep.appimage"), b"").unwrap();
        std::fs::write(bin.join("moneykeep-0.6.0-linux-x86_64.AppImage"), b"").unwrap();
        std::fs::create_dir(bin.join("moneykeep.AppImage")).unwrap();
        let found = linux::appimages_in(app, &[apps.clone(), gear.clone(), bin.clone(), root.path().join("missing")]);
        let names: Vec<_> = found.iter().map(|(p, v)| (p.clone(), v.as_ref().map(ToString::to_string))).collect();
        assert_eq!(
            names,
            [
                (bin.join("moneykeep-0.6.0-linux-x86_64.AppImage"), Some("0.6.0".to_string())),
                (apps.join("moneykeep-0.5.0-linux-x86_64.AppImage"), Some("0.5.0".to_string())),
                (gear.join("moneykeep.appimage"), None),
            ]
        );
    }

    const MONEYKEEP_PUBLISHER: &str = "MoneyKeep Project";

    fn roots() -> Vec<PathBuf> {
        vec![PathBuf::from(r"C:\Program Files"), PathBuf::from(r"C:\Program Files (x86)")]
    }

    #[test]
    fn ledgerkeep_matches_its_former_bookkeep_installer_name() {
        let pdf = &keep_launcher_core::fixtures::RENAMED;
        assert!(is_installer_entry(pdf, "BookKeep", MONEYKEEP_PUBLISHER), "a BookKeep entry from {MONEYKEEP_PUBLISHER} should be LedgerKeep's");
    }

    #[test]
    fn an_install_from_before_the_publisher_rename_is_still_ours() {
        let launcher = &keep_launcher_core::catalog::LAUNCHER;
        assert!(is_installer_entry(launcher, "MoneyKeep Launcher", MONEYKEEP_PUBLISHER));
        assert!(is_installer_entry(launcher, "MoneyKeep Launcher", "MoneyKeep Project Inc"));
        assert!(!is_installer_entry(launcher, "MoneyKeep Launcher", "MoneyKeep Project Incorporated"));
        // A community app names its own publisher; the former MoneyKeep name doesn't stand in for it.
        let community = &keep_launcher_core::fixtures::COMMUNITY;
        if let Some(theirs) = community.msi_publisher {
            assert!(is_installer_entry(community, community.name, theirs));
            assert!(!is_installer_entry(community, community.name, "MoneyKeep Project Inc"));
        }
    }

    #[test]
    fn ledgerkeep_still_matches_its_own_installer_name() {
        let pdf = &keep_launcher_core::fixtures::RENAMED;
        assert!(is_installer_entry(pdf, "LedgerKeep", MONEYKEEP_PUBLISHER));
    }

    #[test]
    fn bookkeep_entries_match_nothing_else() {
        let pdf = &keep_launcher_core::fixtures::RENAMED;
        assert!(!is_installer_entry(pdf, "BookKeep", "Some Other Publisher"));
        assert!(!is_installer_entry(pdf, "MoneyKeep", MONEYKEEP_PUBLISHER));
        for a in SUITE.iter().filter(|a| a.id != "ledgerkeep") {
            assert!(!is_installer_entry(a, "BookKeep", MONEYKEEP_PUBLISHER), "BookKeep matched {}", a.name);
        }
    }

    #[test]
    fn taxkeep_matches_its_own_installer_publisher_only() {
        let sc = &keep_launcher_core::fixtures::COMMUNITY;
        assert!(is_installer_entry(sc, "TaxKeep", "TaxKeep contributors"));
        assert!(!is_installer_entry(sc, "TaxKeep", MONEYKEEP_PUBLISHER));
        assert!(!is_installer_entry(KeepApp::by_id("calckeep").unwrap(), "CalcKeep", "TaxKeep contributors"));
    }

    #[test]
    fn ledgerkeep_exe_is_sought_under_every_name_it_has_shipped_as() {
        let pdf = &keep_launcher_core::fixtures::RENAMED;
        let location = PathBuf::from(r"D:\Apps\BookKeep");
        let roots = roots();
        let mut want = vec![location.join("ledgerkeep.exe"), location.join("bookkeep.exe")];
        for root in &roots {
            for dir in ["LedgerKeep", "BookKeep"] {
                want.push(root.join(dir).join("ledgerkeep.exe"));
                want.push(root.join(dir).join("bookkeep.exe"));
            }
        }
        assert_eq!(exe_candidates(pdf, Some(&location), &roots), want);
    }

    #[test]
    fn apps_without_former_names_match_and_find_their_exe_as_before() {
        let roots = roots();
        let location = PathBuf::from(r"D:\Apps\Somewhere");
        for a in SUITE.iter().filter(|a| a.former_ids.is_empty() && a.former_names.is_empty()) {
            let publisher = a.msi_publisher.unwrap_or(MONEYKEEP_PUBLISHER);
            assert!(is_installer_entry(a, a.name, publisher), "{}", a.name);
            assert!(!is_installer_entry(a, a.name, "Some Other Publisher"), "{}", a.name);
            assert!(!is_installer_entry(a, &a.name.to_lowercase(), publisher), "{}", a.name);
            let exe = format!("{}.exe", a.exe_names()[0]);
            if a.id == "moneykeep" {
                // MoneyKeep's desktop app is moneykeep-app.exe; moneykeep.exe is its command line.
                assert_eq!(exe, "moneykeep-app.exe");
            }
            let in_program_files: Vec<PathBuf> = roots.iter().map(|r| r.join(a.name).join(&exe)).collect();
            assert_eq!(exe_candidates(a, None, &roots), in_program_files);
            let with_location: Vec<PathBuf> = std::iter::once(location.join(&exe)).chain(in_program_files).collect();
            assert_eq!(exe_candidates(a, Some(&location), &roots), with_location);
        }
    }

    /// An app unpacked into the suite folder of a root is found from that root alone, so "Open" works
    /// for a record whose folder is not the one its scope would name.
    #[cfg(windows)]
    #[test]
    fn an_exe_in_the_suite_folder_of_any_root_is_found() {
        let dir = tempfile::tempdir().unwrap();
        let deck = KeepApp::by_id("calckeep").unwrap();
        let folder = dir.path().join("MoneyKeep").join("CalcKeep");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("calckeep.exe"), "x").unwrap();
        assert_eq!(windows::exe_in_roots(deck, vec![dir.path().to_path_buf()]), Some(folder.join("calckeep.exe")));
        assert_eq!(windows::exe_in_roots(deck, vec![dir.path().join("elsewhere")]), None);
    }

    /// A folder the package recorded is searched as it is, whatever it is called, so an app installed
    /// to a folder of the user's choosing is found.
    #[cfg(windows)]
    #[test]
    fn an_exe_in_a_custom_recorded_folder_is_found() {
        let dir = tempfile::tempdir().unwrap();
        let deck = KeepApp::by_id("calckeep").unwrap();
        let custom = dir.path().join("My Slides");
        std::fs::create_dir_all(&custom).unwrap();
        assert_eq!(windows::exe_in_folder(deck, &custom), None);
        std::fs::write(custom.join("calckeep.exe"), "x").unwrap();
        assert_eq!(windows::exe_in_folder(deck, &custom), Some(custom.join("calckeep.exe")));
    }

    /// With two copies in different scopes, the one that is the record's version is chosen, wherever it
    /// was found; with no match, the first found.
    #[cfg(windows)]
    #[test]
    fn the_copy_with_the_records_version_is_the_one_opened() {
        let v = |s: &str| version::parse(s);
        let old = PathBuf::from(r"D:\Old\calckeep.exe");
        let new = PathBuf::from(r"C:\Users\a\AppData\Local\Programs\MoneyKeep\CalcKeep\calckeep.exe");
        let version_of = |p: &std::path::Path| if p == old { v("0.11.1") } else { v("0.12.0") };
        let both = vec![old.clone(), new.clone()];
        assert_eq!(windows::pick_candidate(both.clone(), v("0.12.0").as_ref(), version_of), Some(new.clone()));
        assert_eq!(windows::pick_candidate(both.clone(), v("0.11.1").as_ref(), version_of), Some(old.clone()));
        assert_eq!(windows::pick_candidate(both.clone(), v("0.99.0").as_ref(), version_of), Some(old.clone()));
        assert_eq!(windows::pick_candidate(both, None, version_of), Some(old.clone()));
        assert_eq!(windows::pick_candidate(Vec::new(), v("0.12.0").as_ref(), version_of), None);
    }

    /// Every exe under the given roots, in the order `exe_in_roots` prefers them, so a version match
    /// can see a copy that an earlier root's older copy would otherwise hide.
    #[cfg(windows)]
    #[test]
    fn every_root_is_searched_for_a_version_match() {
        let dir = tempfile::tempdir().unwrap();
        let deck = KeepApp::by_id("calckeep").unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        let exes: Vec<PathBuf> = [&a, &b].iter().map(|r| r.join("MoneyKeep").join("CalcKeep").join("calckeep.exe")).collect();
        for exe in &exes {
            std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
            std::fs::write(exe, "x").unwrap();
        }
        assert_eq!(windows::exe_all_in_roots(deck, vec![a.clone(), b.clone()]), exes);
        assert_eq!(windows::exe_in_roots(deck, vec![a, b]), exes.first().cloned());
        assert!(windows::exe_all_in_roots(deck, vec![dir.path().join("none")]).is_empty());
    }

    #[test]
    fn the_launcher_msi_installs_to_keep_launcher_and_the_apps_do_not() {
        let roots = roots();
        let want: Vec<PathBuf> = roots.iter().map(|root| root.join("MoneyKeep").join("Launcher")).collect();
        assert_eq!(suite_folders(&keep_launcher_core::catalog::LAUNCHER, &roots), want);
        let photo = KeepApp::by_id("moneykeep").unwrap();
        assert!(suite_folders(photo, &roots).is_empty());
    }

    #[test]
    fn the_launcher_records_its_folder_under_the_publisher_key() {
        assert_eq!(
            install_dir_keys(&keep_launcher_core::catalog::LAUNCHER),
            [
                r"SOFTWARE\MoneyKeep Launcher".to_string(),
                format!(r"SOFTWARE\{MONEYKEEP_PUBLISHER}\MoneyKeep Launcher"),
                r"SOFTWARE\MoneyKeep Project Inc\MoneyKeep Launcher".to_string(),
            ]
        );
        let photo = KeepApp::by_id("moneykeep").unwrap();
        assert_eq!(install_dir_keys(photo), [r"SOFTWARE\MoneyKeep".to_string()]);
    }
}
