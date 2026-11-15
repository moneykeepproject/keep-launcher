//! MoneyKeep Launcher updating itself.
//!
//! The launcher is [`catalog::LAUNCHER`](keep_launcher_core::catalog::LAUNCHER) in its own
//! catalog: its releases are `moneykeepproject/keep-launcher`'s, its files are
//! `keep-launcher-<ver>-…`. The new version is downloaded and checked against its published
//! SHA-256 like any MoneyKeep App, then swapped in the way this copy was installed. On Windows a
//! small helper process waits for this launcher to exit (Windows locks a running exe), finishes
//! the swap and starts the new one. The launcher has to exit first anyway: it holds the
//! single-instance lock, and a new copy started while it runs would only ask it to show its window.
//!
//! | Installed as | Swap |
//! |---|---|
//! | Windows MSI (per-user) | after exit, run the new MSI with `/passive`; it reuses the recorded folder, then start the exe |
//! | Windows portable zip | after exit, unzip over this folder (settings in `MoneyKeepLauncherData` stay) |
//! | macOS `.app` | copy the new bundle from the DMG over this one, reopen it (needs the Developer ID team) |
//! | Linux AppImage | rename the new AppImage over this one (fine while running), start it |
//! | Linux `.deb` / `.rpm` | the system package manager, with the password prompt; then restart |
//! | Linux Flatpak bundle | reinstall the bundle on the same installation; then restart |
//! | a loose binary from the tarball (Linux) | rename the new binary over this one, start it |
//!
//! Anything else (FreeBSD, a development build, a Mac build without a signing team) gets the
//! download link instead.
//!
//! **Signatures.** While MoneyKeep Launcher's builds are unsigned (every 0.1.x), an update is
//! trusted on its published SHA-256 alone, the same check every MoneyKeep App install gets (owner
//! decision, 2026-11-14). Once this copy is signed, every file the Windows update starts must be
//! signed by the same publisher.
//!
//! Until 0.1.2 this was delegated to the upstream craft-libs self-update, which only knows the upstream
//! launcher (its release files, registry keys, publisher and bundle id): MoneyKeep Launcher 0.1.0
//! and 0.1.1 couldn't update themselves for that reason.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use keep_launcher_core::catalog::LAUNCHER;
use keep_launcher_core::platform::{Platform, pick_asset};
use keep_launcher_core::protocol::JobPhase;
use keep_launcher_core::release::Release;
use keep_launcher_core::version::{self, Version};
use keep_launcher_core::{KeepApp, PackageKind};

use crate::http::{Http, Stop};
use crate::install::{self, Job};
use crate::paths::Paths;
#[cfg_attr(windows, allow(unused_imports))]
use crate::process;

/// The app this module updates: the launcher's own catalog entry.
const APP: &KeepApp = &LAUNCHER;

/// How this running launcher was installed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Install {
    /// The `MoneyKeep Launcher.app` bundle.
    MacApp(PathBuf),
    /// The per-user Windows Installer package; the exe.
    Msi(PathBuf),
    /// The Windows portable zip; the exe (with `portable.txt` beside it).
    Portable(PathBuf),
    /// The AppImage file.
    AppImage(PathBuf),
    /// A Debian or RPM package; the binary.
    Package(PackageKind, PathBuf),
    /// A Flatpak application installed from the release's bundle.
    Flatpak,
    /// A standalone executable or unpacked tree.
    Standalone(PathBuf),
}

/// The package format updating `install` downloads: the one it was installed from.
pub fn kind(install: &Install) -> PackageKind {
    match install {
        Install::MacApp(_) => PackageKind::Dmg,
        Install::Msi(_) => PackageKind::Msi,
        Install::Portable(_) => PackageKind::PortableZip,
        Install::AppImage(_) => PackageKind::AppImage,
        Install::Package(kind, _) => *kind,
        Install::Flatpak => PackageKind::Flatpak,
        Install::Standalone(_) if cfg!(windows) => PackageKind::PortableZip,
        Install::Standalone(_) => PackageKind::TarGz,
    }
}

/// Makes an installed copy believe it's this version, to try the self-update against the current
/// release. The restart helper drops it, so it lasts one run.
pub const PRETEND_VAR: &str = "KEEP_LAUNCHER_PRETEND_VERSION";

/// Makes a development build believe it's an installed copy, to try the self-update.
pub const DEV_VAR: &str = "KEEP_LAUNCHER_UPDATE_DEV";

/// The version this launcher counts as (see [`PRETEND_VAR`]).
pub fn current_version() -> Option<Version> {
    version_from(std::env::var(PRETEND_VAR).ok().as_deref(), env!("CARGO_PKG_VERSION"))
}

fn version_from(pretend: Option<&str>, package: &str) -> Option<Version> {
    pretend.and_then(version::parse).or_else(|| version::parse(package))
}

/// A pretend version lasts one run, including when an update schedules a restart.
fn clear_pretend_version(cmd: &mut std::process::Command) {
    cmd.env_remove(PRETEND_VAR);
}

/// How long a just-installed version stays exempt from being offered again.
const JUST_UPDATED_FOR: i64 = 24 * 3600;

/// Whether `release` is the version this launcher just updated itself to. Offering it again would
/// loop: update, restart, "update available", … That happens if a release's binary reports a
/// different version from its tag, or while a pretend version is set.
pub fn just_updated_to(paths: &Paths, release: &Release) -> bool {
    let Ok(text) = std::fs::read_to_string(marker(paths)) else { return false };
    let mut lines = text.lines();
    let (Some(tag), Some(at)) = (lines.next(), lines.next().and_then(|a| a.parse::<i64>().ok())) else { return false };
    tag == release.tag && crate::now().checked_sub(at).is_some_and(|age| (0..JUST_UPDATED_FOR).contains(&age))
}

fn marker(paths: &Paths) -> PathBuf {
    paths.data.join("self-update.txt")
}

/// How this copy is installed, if it's one the launcher knows how to replace.
pub fn current() -> Option<Install> {
    let exe = std::env::current_exe().ok()?;
    current_at(&exe, std::env::var_os(DEV_VAR).is_some())
}

/// Whether this installed copy can update itself.
pub fn is_self_updating() -> bool {
    current().is_some()
}

/// [`current`] for a known executable and opt-in setting, without reading the ambient
/// environment.
fn current_at(exe: &Path, opted_in: bool) -> Option<Install> {
    let exe = exe.canonicalize().unwrap_or_else(|_| exe.to_path_buf());
    if is_development_build(&exe, opted_in) {
        return None;
    }
    detect(&exe)
}

/// Cargo's cache-directory marker at the root of a target directory.
const CARGO_CACHE_MARKER: &str = "CACHEDIR.TAG";

/// Whether `exe` runs from a Cargo build tree, wherever `CARGO_TARGET_DIR` or
/// `build.target-dir` put it. Cargo caches rustc information in `.rustc_info.json` at the target
/// root, which identifies the tree. Without it (`CARGO_CACHE_RUSTC_INFO=0`, or a tree copied
/// without the hidden file) a profile directory with Cargo's `.fingerprint`, `deps` and `build`
/// subdirectories counts when its target root carries `CACHEDIR.TAG`.
fn is_development_build(exe: &Path, opted_in: bool) -> bool {
    !opted_in && exe.ancestors().skip(1).any(is_cargo_build_dir)
}

fn is_cargo_build_dir(dir: &Path) -> bool {
    if dir.join(".rustc_info.json").is_file() {
        return true;
    }
    let Some(profile_parent) = dir.parent() else { return false };
    let is_profile = [".fingerprint", "deps", "build"].into_iter().all(|name| dir.join(name).is_dir());
    if !is_profile {
        return false;
    }
    // `<target>/<profile>`, or `<target>/<triple>/<profile>` for cross-compiled output.
    profile_parent.join(CARGO_CACHE_MARKER).is_file() || profile_parent.parent().is_some_and(|root| root.join(CARGO_CACHE_MARKER).is_file())
}

/// The macOS bundle. Without MoneyKeep's Developer ID team the new bundle can't pass the checks
/// every installed app must (see `install::macos`), so such a build offers the download instead.
#[cfg(target_os = "macos")]
fn detect(exe: &Path) -> Option<Install> {
    APP.mac_team?;
    let bundle = exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app"))?;
    let plist = plist::Value::from_file(bundle.join("Contents/Info.plist")).ok()?;
    let id = plist.as_dictionary().and_then(|d| d.get("CFBundleIdentifier")).and_then(|v| v.as_string())?;
    (id == APP.bundle_id()).then(|| Install::MacApp(bundle.to_path_buf()))
}

/// How the Windows copy at `exe` was installed: portable (a `portable.txt` beside it), by the
/// launcher's MSI (the folder it records, or its `App Paths` entry), else a loose folder.
#[cfg(windows)]
fn detect(exe: &Path) -> Option<Install> {
    let dir = exe.parent()?;
    if dir.join("portable.txt").is_file() {
        return Some(Install::Portable(exe.to_path_buf()));
    }
    if windows::is_msi_copy(exe) {
        return Some(Install::Msi(exe.to_path_buf()));
    }
    exe.is_file().then(|| Install::Standalone(exe.to_path_buf()))
}

#[cfg(target_os = "linux")]
fn detect(exe: &Path) -> Option<Install> {
    if process::in_sandbox() {
        // A copy installed from the release's bundle updates itself on the same installation. A
        // copy from a repository is that repository's to update.
        let level = flatpak_level()?;
        return from_bundle(level).then_some(Install::Flatpak);
    }
    if let Some(appimage) = std::env::var_os("APPIMAGE") {
        return Some(Install::AppImage(PathBuf::from(appimage)));
    }
    let t = std::time::Duration::from_secs(10);
    if process::capture("dpkg-query", &["-W", "-f", "${db:Status-Status}", APP.id], t).is_some_and(|s| s.trim() == "installed") {
        return Some(Install::Package(PackageKind::Deb, exe.to_path_buf()));
    }
    if process::capture("rpm", &["-q", APP.id], t).is_some() {
        return Some(Install::Package(PackageKind::Rpm, exe.to_path_buf()));
    }
    exe.is_file().then(|| Install::Standalone(exe.to_path_buf()))
}

#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
fn detect(_exe: &Path) -> Option<Install> {
    None
}

/// Where this sandbox's Flatpak copy is installed, from `app-path` in `/.flatpak-info`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FlatpakLevel {
    User,
    System,
}

#[cfg(target_os = "linux")]
fn flatpak_level() -> Option<FlatpakLevel> {
    parse_flatpak_level(&std::fs::read_to_string("/.flatpak-info").ok()?)
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_flatpak_level(info: &str) -> Option<FlatpakLevel> {
    let mut in_instance = false;
    for line in info.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_instance = line == "[Instance]";
        } else if in_instance && let Some(path) = line.strip_prefix("app-path=") {
            // The host's path. The per-user installation is ~/.local/share/flatpak there; it can't
            // be looked up from in here, where XDG_DATA_HOME points into ~/.var/app instead.
            let path = path.trim();
            if path.contains("/.local/share/flatpak/") {
                return Some(FlatpakLevel::User);
            }
            if path.contains("/var/lib/flatpak/") {
                return Some(FlatpakLevel::System);
            }
            return None;
        }
    }
    None
}

/// Whether this copy was installed from a `.flatpak` bundle, asked of the host's Flatpak on the
/// installation the copy runs from.
#[cfg(target_os = "linux")]
fn from_bundle(level: FlatpakLevel) -> bool {
    let scope = match level {
        FlatpakLevel::User => "--user",
        FlatpakLevel::System => "--system",
    };
    process::capture_flatpak(&["info", scope, "--show-origin", &APP.bundle_id()], std::time::Duration::from_secs(3))
        .is_some_and(|origin| is_bundle_origin(origin.trim()))
}

/// Flatpak names a bundle's origin after the last part of the app id (`keep-launcher`), numbered
/// when that name is taken: `keep-launcher-origin`, `keep-launcher1-origin`, …
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn is_bundle_origin(origin: &str) -> bool {
    origin.strip_prefix(APP.id).and_then(|rest| rest.strip_suffix("-origin")).is_some_and(|n| n.bytes().all(|b| b.is_ascii_digit()))
}

/// The release file that updating `install` on `platform` downloads, when the release has it.
pub fn asset_for<'a>(install: &Install, release: &'a Release, platform: Platform) -> Option<&'a keep_launcher_core::Asset> {
    let stems: Vec<&str> = APP.stems().collect();
    pick_asset(release, &stems, platform, &[kind(install)]).map(|(asset, _)| asset)
}

/// Whether `release` has the file that updating `install` on `platform` downloads.
pub fn installs_from(install: &Install, release: &Release, platform: Platform) -> bool {
    asset_for(install, release, platform).is_some()
}

/// Downloads `release` for `platform`, puts it in place and starts the helper that relaunches
/// it once this process has exited. On success the caller must quit promptly. `last` is called
/// once the download is ready and returns when the launcher may replace itself and restart.
#[allow(clippy::too_many_arguments)]
pub fn update(
    install: &Install,
    release: &Release,
    platform: Platform,
    http: &Http,
    paths: &Paths,
    stop: &Stop,
    phase: &mut dyn FnMut(JobPhase),
    last: &dyn Fn() -> Result<(), String>,
) -> Result<(), String> {
    if let (Some(rel_v), Some(cur_v)) = (release.version.as_ref(), current_version())
        && !version::is_newer(rel_v, &cur_v)
    {
        return Err(format!("{} {} is not newer than the running version {}", APP.name, release.version_label(), cur_v));
    }
    let asset = asset_for(install, release, platform)
        .ok_or_else(|| format!("{} {} has no {} for {}", APP.name, release.version_label(), kind(install).label(), platform.label()))?
        .clone();
    install::bind_artifact_version(APP, release, &asset)?;
    let mut job = Job { http, paths, stop, phase, install_locations: &Default::default(), replace: false, downgrade: None };
    let (file, _sha) = install::download(release, &asset, &mut job)?;
    if let Err(e) = precheck(install, paths, &file) {
        let _ = std::fs::remove_file(&file);
        return Err(e);
    }
    // The restart ends this process: nothing else may be under way.
    if let Err(e) = last() {
        let _ = std::fs::remove_file(&file);
        return Err(e);
    }
    phase(JobPhase::Installing);
    // Written before the swap starts: the Windows helper runs alongside this process and clears
    // it when it fails, which a write after scheduling could undo.
    mark_installed(paths, release);
    let scheduled = swap(install, release, &file, paths, stop, phase);
    unmark_unless_scheduled(paths, scheduled)
}

/// What the Windows helper will check, checked while the launcher still runs and can show the error.
#[cfg(windows)]
fn precheck(install: &Install, paths: &Paths, file: &Path) -> Result<(), String> {
    windows::precheck(install, paths, file)
}

#[cfg(not(windows))]
fn precheck(_install: &Install, _paths: &Paths, _file: &Path) -> Result<(), String> {
    Ok(())
}

/// Puts the downloaded `file` in place of `install` and schedules the restart. Each platform uses
/// only some of the arguments.
#[allow(unused_variables)]
fn swap(install: &Install, release: &Release, file: &Path, paths: &Paths, stop: &Stop, phase: &mut dyn FnMut(JobPhase)) -> Result<(), String> {
    let pid = std::process::id();
    match install {
        #[cfg(target_os = "macos")]
        Install::MacApp(bundle) => {
            let existing = keep_launcher_core::Installed {
                version: None,
                source: keep_launcher_core::InstallSource::MacApp,
                location: bundle.display().to_string(),
                package: None,
                externally_managed: false,
                origin: None,
                installation: None,
            };
            let placed = install::macos::install_dmg(APP, file, Some(&existing), None, false, paths, &install::macos::Expect::release(APP, release));
            let _ = std::fs::remove_file(file);
            let placed = placed?;
            relaunch_after_exit(pid, &[Path::new("/usr/bin/open").as_os_str(), placed.as_os_str()])
        }
        #[cfg(target_os = "linux")]
        Install::AppImage(path) => {
            let result = replace_appimage(file, path);
            let _ = std::fs::remove_file(file);
            result?;
            relaunch_after_exit(pid, &[path.as_os_str()])
        }
        #[cfg(target_os = "linux")]
        Install::Package(kind, exe) => {
            let result = install::linux::install_package(*kind, file, None, stop, phase);
            let _ = std::fs::remove_file(file);
            result?;
            relaunch_after_exit(pid, &[exe.as_os_str()])
        }
        #[cfg(target_os = "linux")]
        Install::Flatpak => {
            // Re-read rather than trusting a stale guess: `--user` on a system copy would install a
            // second one beside it.
            let installation = match flatpak_level() {
                Some(FlatpakLevel::System) => "system",
                Some(FlatpakLevel::User) => "user",
                None => {
                    let _ = std::fs::remove_file(file);
                    return Err(cannot_update());
                }
            };
            let result = install::linux::install_package(PackageKind::Flatpak, file, Some(installation), stop, phase);
            let _ = std::fs::remove_file(file);
            result?;
            relaunch_flatpak_after_exit()
        }
        #[cfg(windows)]
        Install::Msi(exe) => windows::helper(paths, pid, exe, &Finish::Msi(install::msi_path(file)?)),
        #[cfg(windows)]
        Install::Portable(exe) | Install::Standalone(exe) => {
            exe.parent().ok_or_else(|| format!("{} isn't in a folder", APP.name))?;
            windows::helper(paths, pid, exe, &Finish::Portable { zip: file.to_path_buf(), tmp: paths.cache.join(PORTABLE_STAGING) })
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        Install::Standalone(exe) => {
            let result = replace_standalone(file, exe);
            let _ = std::fs::remove_file(file);
            result?;
            relaunch_after_exit(pid, &[exe.as_os_str()])
        }
        #[allow(unreachable_patterns)]
        _ => {
            let _ = std::fs::remove_file(file);
            Err(cannot_update())
        }
    }
}

fn cannot_update() -> String {
    format!("this copy of {} can't update itself; download the new version instead", APP.name)
}

/// Records `release` as just installed, so it isn't offered again for [`JUST_UPDATED_FOR`].
fn mark_installed(paths: &Paths, release: &Release) {
    let _ = std::fs::create_dir_all(&paths.data);
    if let Err(e) = crate::paths::write_atomic(&marker(paths), format!("{}\n{}\n", release.tag, crate::now()).as_bytes()) {
        log::warn!("couldn't record the self-update: {e}");
    }
}

/// Removes the record of [`mark_installed`] when the swap couldn't be scheduled, so the update
/// stays on offer.
fn unmark_unless_scheduled(paths: &Paths, scheduled: Result<(), String>) -> Result<(), String> {
    if scheduled.is_err() {
        let _ = std::fs::remove_file(marker(paths));
    }
    scheduled
}

/// Copies the new AppImage beside the running one and renames it over it. The running copy is
/// mounted from its own (now unlinked) file, so it keeps working until it exits.
#[cfg(target_os = "linux")]
fn replace_appimage(new: &Path, path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let dir = path.parent().ok_or("the AppImage isn't in a folder")?;
    let staged = dir.join(format!(".{}.update", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
    std::fs::copy(new, &staged).map_err(|e| format!("couldn't write next to {} ({e})", path.display()))?;
    // The replacement must be executable; 0o755 is not group/world-writable.
    std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?; // NOSONAR
    std::fs::rename(&staged, path).map_err(|e| {
        let _ = std::fs::remove_file(&staged);
        format!("couldn't replace {} ({e})", path.display())
    })
}

/// Extracts the launcher's binary (`keep-launcher`) from a release tarball and renames it over
/// the running standalone executable.
#[cfg(all(unix, not(target_os = "macos")))]
fn replace_standalone(tarball: &Path, exe: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let dir = exe.parent().ok_or_else(|| "the executable isn't in a folder".to_string())?;
    let f = std::fs::File::open(tarball).map_err(|e| format!("couldn't open {} ({e})", tarball.display()))?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(f));
    let staged = dir.join(format!(".{}.update", exe.file_name().and_then(|n| n.to_str()).unwrap_or(APP.id)));
    let mut found = false;
    for entry in archive.entries().map_err(|e| format!("couldn't read the release archive ({e})"))? {
        let mut entry = entry.map_err(|e| format!("couldn't read the release archive ({e})"))?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let is_binary = entry.path().map_err(|e| format!("couldn't read the release archive ({e})"))?.file_name().is_some_and(|n| n == APP.id);
        if is_binary {
            let mut out = std::fs::File::create(&staged).map_err(|e| format!("couldn't create {} ({e})", staged.display()))?;
            if let Err(e) = std::io::copy(&mut entry, &mut out) {
                let _ = std::fs::remove_file(&staged);
                return Err(format!("couldn't unpack the new version ({e})"));
            }
            found = true;
            break;
        }
    }
    if !found {
        let _ = std::fs::remove_file(&staged);
        return Err(format!("the release archive has no {} program", APP.id));
    }
    // 0o755 is not group/world-writable.
    std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?; // NOSONAR
    std::fs::rename(&staged, exe).map_err(|e| {
        let _ = std::fs::remove_file(&staged);
        format!("couldn't replace {} ({e})", exe.display())
    })
}

/// Starts `sh`, which waits for `pid` to exit and then runs `command`. Detached, so it outlives us.
#[cfg(unix)]
fn relaunch_after_exit(pid: u32, command: &[&std::ffi::OsStr]) -> Result<(), String> {
    process::spawn_detached(helper_command(pid, command)).map_err(|e| format!("couldn't schedule the restart ({e})"))
}

#[cfg(unix)]
fn helper_command(pid: u32, command: &[&std::ffi::OsStr]) -> std::process::Command {
    let mut cmd = process::command("/bin/sh");
    cmd.arg("-c")
        .arg(r#"pid=$1; shift; i=0; while kill -0 "$pid" 2>/dev/null && [ $i -lt 600 ]; do sleep 0.1; i=$((i+1)); done; exec "$@""#)
        .arg("sh")
        .arg(pid.to_string())
        .args(command);
    clear_pretend_version(&mut cmd);
    cmd
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_flatpak_instance_id(info: &str) -> Option<String> {
    info.lines().filter_map(|line| line.trim().strip_prefix("instance-id=")).map(str::trim).find(|id| !id.is_empty()).map(str::to_string)
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_flatpak_host_pid(ps_output: &str, target_instance: &str) -> Option<u32> {
    ps_output.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        match (parts.next(), parts.next()) {
            (Some(instance), Some(pid)) if instance == target_instance => pid.parse().ok(),
            _ => None,
        }
    })
}

/// Starts a host helper that waits for this Flatpak instance to exit, then starts it again through
/// Flatpak. If the old copy is still running after a minute the helper gives up rather than start
/// a second one.
#[cfg(target_os = "linux")]
fn relaunch_flatpak_after_exit() -> Result<(), String> {
    let instance_id = std::fs::read_to_string("/.flatpak-info").ok().as_deref().and_then(parse_flatpak_instance_id);
    let host_pid = instance_id.as_deref().and_then(|id| {
        let out = process::capture_flatpak(&["ps", "--columns=instance,pid"], std::time::Duration::from_secs(3))?;
        parse_flatpak_host_pid(&out, id)
    });
    let (wait_clause, target) = match (host_pid, instance_id) {
        (Some(pid), _) => (
            r#"pid=$1; shift; i=0; while kill -0 "$pid" 2>/dev/null; do [ $i -ge 600 ] && exit 1; sleep 0.1; i=$((i+1)); done; exec "$@""#,
            pid.to_string(),
        ),
        (None, Some(id)) => (
            r#"id=$1; shift; i=0; while flatpak ps --columns=instance 2>/dev/null | grep -q "^$id$"; do [ $i -ge 600 ] && exit 1; sleep 0.1; i=$((i+1)); done; exec "$@""#,
            id,
        ),
        (None, None) => return Err(format!("{} is updated; quit and reopen it to start the new version", APP.name)),
    };
    let script = format!("({wait_clause}) &");
    // `flatpak run` reapplies persistent overrides, so the pretend version is unset there too.
    let unset = format!("--unset-env={PRETEND_VAR}");
    let mut cmd = process::command("flatpak-spawn");
    cmd.args(["--host", "sh", "-c", &script, "sh", &target, "flatpak", "run", &unset, &APP.bundle_id()]);
    clear_pretend_version(&mut cmd);
    process::spawn_detached(cmd).map_err(|e| format!("couldn't schedule the Flatpak restart ({e})"))
}

/// The first argument of the Windows update helper: a copy of this program that finishes an
/// update once the launcher has exited (see [`finish`]).
pub const FINISH_FLAG: &str = "--finish-update";

/// What the update helper does once the old launcher has exited.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
enum Finish {
    /// Run the new MSI, which reinstalls into the folder the old one recorded.
    Msi(PathBuf),
    /// Unpack the new portable zip into `tmp` and copy it over the launcher's folder.
    Portable { zip: PathBuf, tmp: PathBuf },
}

/// The helper's arguments: [`FINISH_FLAG`], the process id to wait for, the exe to start
/// afterwards, then the step.
#[cfg_attr(not(windows), allow(dead_code))]
fn helper_args(pid: u32, exe: &Path, finish: &Finish) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec![FINISH_FLAG.into(), pid.to_string().into(), exe.into()];
    match finish {
        Finish::Msi(msi) => args.extend(["msi".into(), msi.into()]),
        Finish::Portable { zip, tmp } => args.extend(["portable".into(), zip.into(), tmp.into()]),
    }
    args
}

/// Reads [`helper_args`] back.
#[cfg_attr(not(windows), allow(dead_code))]
fn parse_helper_args(args: &[OsString]) -> Option<(u32, PathBuf, Finish)> {
    let [flag, pid, exe, step @ ..] = args else { return None };
    if flag != FINISH_FLAG {
        return None;
    }
    let pid = pid.to_str()?.parse().ok()?;
    let finish = match step {
        [kind, msi] if kind == "msi" => Finish::Msi(msi.into()),
        [kind, zip, tmp] if kind == "portable" => Finish::Portable { zip: zip.into(), tmp: tmp.into() },
        _ => return None,
    };
    Some((pid, exe.into(), finish))
}

/// The cache folder the update helper is copied into.
#[cfg_attr(not(windows), allow(dead_code))]
const HELPER_DIR: &str = "launcher-update-helper";
/// The cache folder a portable update is unpacked into.
#[cfg_attr(not(windows), allow(dead_code))]
const PORTABLE_STAGING: &str = "launcher-update";

/// Finishes an update when this program was started as the update helper; `args` are its
/// arguments without the program name. `None` when they don't start with [`FINISH_FLAG`]. The
/// launcher is started again even when the step fails: the old copy is then still in place.
#[cfg(windows)]
pub fn finish(args: &[OsString]) -> Option<Result<(), String>> {
    if args.first().is_none_or(|a| a != FINISH_FLAG) {
        return None;
    }
    Some(windows::finish(args))
}

/// Finishes an update when this program was started as the update helper (Windows only).
#[cfg(not(windows))]
pub fn finish(_args: &[OsString]) -> Option<Result<(), String>> {
    None
}

/// Where a failed update's error waits for the next start.
fn failure_file(paths: &Paths) -> PathBuf {
    paths.logs().join("self-update-error.txt")
}

/// Records a failed update: the next check offers the update again, and the next start logs `error`.
#[cfg_attr(not(windows), allow(dead_code))]
fn record_failure(paths: &Paths, error: &str) {
    let _ = std::fs::remove_file(marker(paths));
    let _ = std::fs::create_dir_all(paths.logs());
    let _ = crate::paths::write_atomic(&failure_file(paths), error.as_bytes());
}

/// The error of the last update, if it failed, once.
pub fn take_failure(paths: &Paths) -> Option<String> {
    let file = failure_file(paths);
    let error = std::fs::read_to_string(&file).ok()?;
    let _ = std::fs::remove_file(&file);
    Some(error)
}

/// Whether `exe` is named like the launcher's own executable (`keep-launcher.exe`, any case).
#[cfg_attr(not(windows), allow(dead_code))]
fn is_own_executable(exe: &Path) -> bool {
    exe.file_stem().and_then(|stem| stem.to_str()).is_some_and(|stem| APP.stems().any(|id| id.eq_ignore_ascii_case(stem)))
}

#[cfg(windows)]
mod windows {
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    use super::{APP, Finish, HELPER_DIR, Install, PORTABLE_STAGING, helper_args, is_own_executable, parse_helper_args, record_failure};
    use crate::paths::Paths;
    use crate::process;

    /// Where the MSI records its folder (`InstallDir`), current publisher first. Packages before
    /// 0.1.0-alpha.1 used `MoneyKeep Project Inc`.
    const MSI_KEYS: [&str; 2] = [r"Software\MoneyKeep Project\MoneyKeep Launcher", r"Software\MoneyKeep Project Inc\MoneyKeep Launcher"];
    /// The per-user `App Paths` entry the MSI writes for the exe.
    const APP_PATHS: &str = r"Software\Microsoft\Windows\CurrentVersion\App Paths\keep-launcher.exe";

    fn canonical(path: &Path) -> PathBuf {
        dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
    }

    fn same_path(a: &Path, b: &Path) -> bool {
        canonical(a).as_os_str().to_string_lossy().eq_ignore_ascii_case(&canonical(b).as_os_str().to_string_lossy())
    }

    /// Whether `exe` is the copy the launcher's per-user MSI installed: its folder is the one the
    /// MSI recorded, or the MSI's `App Paths` entry names it.
    pub(super) fn is_msi_copy(exe: &Path) -> bool {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let recorded = exe.parent().is_some_and(|dir| {
            MSI_KEYS.iter().any(|key| {
                hkcu.open_subkey(key)
                    .and_then(|k| k.get_value::<String, _>("InstallDir"))
                    .is_ok_and(|value| !value.trim().is_empty() && same_path(Path::new(value.trim()), dir))
            })
        });
        recorded
            || hkcu
                .open_subkey(APP_PATHS)
                .and_then(|k| k.get_value::<String, _>(""))
                .is_ok_and(|value| app_path_value(&value).is_some_and(|target| same_path(&target, exe)))
    }

    /// An `App Paths` default value as a path: Windows allows it in quotes.
    fn app_path_value(value: &str) -> Option<PathBuf> {
        let value = value.trim().trim_matches('"').trim();
        (!value.is_empty()).then(|| PathBuf::from(value))
    }

    /// The launcher's folders as the helper finds them again from `exe`.
    fn paths_for(exe: &Path) -> Result<Paths, String> {
        Paths::for_exe(exe)
    }

    /// Checks, while the launcher still runs and can show the error, what the helper checks before
    /// it installs: the files are signed like this program (see [`signed_like`]), the exe is the
    /// launcher's own, and the helper will find the same cache.
    pub(super) fn precheck(install: &Install, paths: &Paths, download: &Path) -> Result<(), String> {
        let me = std::env::current_exe().map_err(|e| format!("couldn't find this program ({e})"))?;
        match install {
            Install::Msi(exe) | Install::Portable(exe) | Install::Standalone(exe) => signed_like(&me, exe)
                .and_then(|()| if matches!(install, Install::Msi(_)) { signed_like(&me, download) } else { Ok(()) })
                .and_then(|()| own_executable(exe))
                .and_then(|()| same_cache_as_helper(paths, exe)),
            _ => Ok(()),
        }
    }

    fn own_executable(exe: &Path) -> Result<(), String> {
        if is_own_executable(exe) { Ok(()) } else { Err(format!("the update helper only finishes {}'s own update", APP.name)) }
    }

    /// Refuses a launcher that keeps its files where the helper wouldn't find them again.
    fn same_cache_as_helper(paths: &Paths, exe: &Path) -> Result<(), String> {
        let theirs = paths_for(exe).ok().and_then(|helper| helper.cache.canonicalize().ok());
        if theirs.is_some() && theirs == paths.cache.canonicalize().ok() {
            Ok(())
        } else {
            Err(format!("{} can't update itself while it keeps its files in {}; download the new version instead", APP.name, paths.cache.display()))
        }
    }

    /// Accepts `file` when this program (`reference`) is unsigned, as every 0.1.x build is: the
    /// update was already checked against its published SHA-256 (owner decision, 2026-11-14). Once
    /// `reference` carries an Authenticode signature, `file` must carry one from the same publisher.
    fn signed_like(reference: &Path, file: &Path) -> Result<(), String> {
        let publisher = |path: &Path| verifysign::verify_file(path).map(|signature| signature.subject_name());
        let Ok(ours) = publisher(reference) else { return Ok(()) };
        let theirs = publisher(file).map_err(|e| format!("{} isn't signed ({e})", file.display()))?;
        if ours == theirs { Ok(()) } else { Err(format!("{} isn't signed by {}'s publisher", file.display(), APP.name)) }
    }

    /// Whether `file` is a Windows program or library (it starts with the `MZ` header).
    fn is_program(file: &Path) -> bool {
        use std::io::Read;
        let mut magic = [0u8; 2];
        std::fs::File::open(file).and_then(|mut f| f.read_exact(&mut magic)).is_ok() && magic == *b"MZ"
    }

    /// Copies this program into the cache and starts the copy as the update helper, which waits for
    /// `pid` to exit, does `finish` and starts `exe`. Run from the cache, the helper doesn't hold
    /// the installed exe that the step replaces.
    pub(super) fn helper(paths: &Paths, pid: u32, exe: &Path, finish: &Finish) -> Result<(), String> {
        let program = std::env::current_exe().map_err(|e| format!("couldn't find this program ({e})"))?;
        let dir = paths.cache.join(HELPER_DIR);
        // The previous update's copy, when it's still there.
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| format!("couldn't schedule the update ({e})"))?;
        let helper = dir.join(program.file_name().ok_or("this program has no file name")?);
        std::fs::copy(&program, &helper).map_err(|e| format!("couldn't schedule the update ({e})"))?;
        let mut cmd = process::command(&helper);
        cmd.args(helper_args(pid, exe, finish));
        super::clear_pretend_version(&mut cmd);
        process::spawn_detached(cmd).map_err(|e| format!("couldn't schedule the update ({e})"))
    }

    /// The helper's work, once [`super::finish`] has seen its flag.
    pub(super) fn finish(args: &[OsString]) -> Result<(), String> {
        let (pid, exe, step) = parse_helper_args(args).ok_or("the update helper's arguments are incomplete")?;
        let me = std::env::current_exe().map_err(|_| "the update helper couldn't find itself".to_string())?;
        let paths = paths_for(&exe)?;
        // Nothing runs unless this is the launcher's own update, signed like this helper.
        if let Err(e) = check_helper(&me, &paths, &exe, &step).and_then(|()| signed_like(&me, &exe)) {
            record_failure(&paths, &e);
            return Err(e);
        }
        let done = run_step(&me, pid, &exe, &step, std::time::Duration::from_secs(60));
        if let Err(e) = &done {
            record_failure(&paths, e);
        }
        // The new launcher, or the old one when the step failed. The per-user MSI reinstalls into
        // the folder it recorded, so the exe keeps its path.
        if !exe.is_file() {
            return done;
        }
        let started = signed_like(&me, &exe)
            .and_then(|()| process::spawn_detached(std::process::Command::new(&exe)).map_err(|e| format!("couldn't start {} ({e})", exe.display())));
        if let Err(e) = &started {
            record_failure(&paths, e);
        }
        done.and(started)
    }

    /// Allows only the launcher's own update: `exe` is the launcher's MSI or portable copy, of the
    /// kind `step` updates, and the helper (`me`), the download and the staging folder are in that
    /// copy's cache. Anything else is refused before the helper runs or starts a file.
    fn check_helper(me: &Path, paths: &Paths, exe: &Path, step: &Finish) -> Result<(), String> {
        let refused = || format!("the update helper only finishes {}'s own update", APP.name);
        let canon = |p: &Path| p.canonicalize().map_err(|_| refused());
        let cache = canon(&paths.cache)?;
        if canon(me.parent().ok_or_else(refused)?)? != cache.join(HELPER_DIR) {
            return Err(refused());
        }
        let exe = canon(exe)?;
        if !is_own_executable(&exe) {
            return Err(refused());
        }
        let downloads = canon(&paths.downloads())?;
        let in_downloads = |file: &Path| canon(file).map(|f| f.parent() == Some(downloads.as_path()));
        let allowed = match (super::detect(&exe), step) {
            (Some(Install::Msi(installed)), Finish::Msi(msi)) => installed == exe && in_downloads(msi)?,
            (Some(Install::Portable(installed) | Install::Standalone(installed)), Finish::Portable { zip, tmp }) => {
                installed == exe
                    && in_downloads(zip)?
                    && tmp.file_name().is_some_and(|n| n == PORTABLE_STAGING)
                    && canon(tmp.parent().ok_or_else(refused)?)? == cache
            }
            _ => false,
        };
        if allowed { Ok(()) } else { Err(refused()) }
    }

    /// Waits up to `wait` for `pid` to exit, does `step` once it has, and removes the download
    /// either way.
    fn run_step(reference: &Path, pid: u32, exe: &Path, step: &Finish, wait: std::time::Duration) -> Result<(), String> {
        let result = if process::wait_for_exit(pid, wait) {
            match step {
                Finish::Msi(msi) => signed_like(reference, msi).and_then(|()| crate::install::windows::reinstall(msi)),
                Finish::Portable { zip, tmp } => match (exe.parent(), exe.file_name()) {
                    (Some(dir), Some(executable)) => crate::portable::unpack_over(zip, tmp, dir, executable, &|file| {
                        // The launcher's own exe is checked even when its contents aren't a PE file.
                        if file.file_name() == Some(executable) || is_program(file) { signed_like(reference, file) } else { Ok(()) }
                    }),
                    _ => Err(format!("{} isn't in a folder", APP.name)),
                },
            }
        } else {
            Err(format!("{} was still running after a minute, so the update wasn't installed", APP.name))
        };
        match step {
            Finish::Msi(msi) => {
                let _ = std::fs::remove_file(msi);
            }
            // `unpack_over` removes the staging folder, unless it holds files it couldn't put back.
            Finish::Portable { zip, .. } => {
                let _ = std::fs::remove_file(zip);
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use keep_launcher_core::release::Asset;
    use keep_launcher_core::{Arch, Os};

    use super::*;

    /// MoneyKeep Launcher v0.1.1's files, as its release workflow names them.
    fn release_0_1_1() -> Release {
        let names = [
            "keep-launcher-0.1.1-windows-x64.msi",
            "keep-launcher-0.1.1-windows-x64-portable.zip",
            "keep-launcher-0.1.1-windows-arm64.msi",
            "keep-launcher-0.1.1-windows-arm64-portable.zip",
            "keep-launcher-0.1.1-macos-universal.dmg",
            "keep-launcher-0.1.1-linux-x86_64.AppImage",
            "keep-launcher-0.1.1-linux-x86_64.AppImage.zsync",
            "keep-launcher-0.1.1-linux-x86_64.deb",
            "keep-launcher-0.1.1-linux-x86_64.rpm",
            "keep-launcher-0.1.1-linux-x86_64.tar.gz",
            "keep-launcher-0.1.1-linux-x86_64.flatpak",
            "keep-launcher-0.1.1-freebsd-x86_64.tar.gz",
            "keep-launcher-web-0.1.1.zip",
            "SHA256SUMS.txt",
        ];
        Release {
            tag: "v0.1.1".into(),
            version: version::parse("0.1.1"),
            assets: names
                .iter()
                .map(|n| Asset {
                    name: (*n).into(),
                    size: 1,
                    url: format!("https://github.com/moneykeepproject/keep-launcher/releases/download/v0.1.1/{n}"),
                    sha256: None,
                })
                .collect(),
            ..Release::default()
        }
    }

    #[test]
    fn the_launcher_updates_as_moneykeep_launcher_from_its_own_repository() {
        assert_eq!(APP.id, "keep-launcher");
        assert_eq!(APP.name, "MoneyKeep Launcher");
        assert_eq!(APP.repo, "moneykeepproject/keep-launcher");
        assert_eq!(APP.bundle_id(), "com.moneykeep.keep-launcher");
    }

    #[test]
    fn every_install_finds_its_file_in_a_real_release() {
        let release = release_0_1_1();
        let windows = Platform { os: Os::Windows, arch: Arch::X64 };
        let pick = |install: &Install, platform| asset_for(install, &release, platform).map(|a| a.name.as_str());
        assert_eq!(pick(&Install::Msi(PathBuf::new()), windows), Some("keep-launcher-0.1.1-windows-x64.msi"));
        assert_eq!(pick(&Install::Portable(PathBuf::new()), windows), Some("keep-launcher-0.1.1-windows-x64-portable.zip"));
        let mac = Platform { os: Os::MacOs, arch: Arch::Arm64 };
        assert_eq!(pick(&Install::MacApp(PathBuf::new()), mac), Some("keep-launcher-0.1.1-macos-universal.dmg"));
        let linux = Platform { os: Os::Linux, arch: Arch::X64 };
        assert_eq!(pick(&Install::AppImage(PathBuf::new()), linux), Some("keep-launcher-0.1.1-linux-x86_64.AppImage"));
        assert_eq!(pick(&Install::Package(PackageKind::Deb, PathBuf::new()), linux), Some("keep-launcher-0.1.1-linux-x86_64.deb"));
        assert_eq!(pick(&Install::Package(PackageKind::Rpm, PathBuf::new()), linux), Some("keep-launcher-0.1.1-linux-x86_64.rpm"));
        assert_eq!(pick(&Install::Flatpak, linux), Some("keep-launcher-0.1.1-linux-x86_64.flatpak"));
        if !cfg!(windows) {
            assert_eq!(pick(&Install::Standalone(PathBuf::new()), linux), Some("keep-launcher-0.1.1-linux-x86_64.tar.gz"));
        }
        assert!(!installs_from(&Install::Msi(PathBuf::new()), &release, Platform { os: Os::Windows, arch: Arch::X86 }));
        for install in [Install::Msi(PathBuf::new()), Install::Portable(PathBuf::new())] {
            let asset = asset_for(&install, &release, windows).unwrap();
            install::bind_artifact_version(APP, &release, asset).unwrap();
        }
    }

    #[test]
    fn another_launchers_files_are_never_picked() {
        let mut release = release_0_1_1();
        for asset in &mut release.assets {
            asset.name = asset.name.replace("keep-launcher", concat!("artcraft", "-launcher"));
        }
        let windows = Platform { os: Os::Windows, arch: Arch::X64 };
        assert!(!installs_from(&Install::Msi(PathBuf::new()), &release, windows));
        assert!(!installs_from(&Install::Portable(PathBuf::new()), &release, windows));
    }

    #[test]
    fn a_missing_file_names_moneykeep_launcher_and_the_format() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path());
        let http = Http::new().unwrap();
        let release = Release { tag: "v9.0.0".into(), version: version::parse("9.0.0"), ..Release::default() };
        let platform = Platform { os: Os::Windows, arch: Arch::X64 };
        let error = update(&Install::Msi(PathBuf::new()), &release, platform, &http, &paths, &Stop::default(), &mut |_| {}, &|| Ok(())).unwrap_err();
        assert_eq!(error, "MoneyKeep Launcher 9.0.0 has no Installer (.msi) for Windows x64");
        assert!(!error.to_lowercase().contains("craft"));
    }

    #[test]
    fn an_older_release_is_not_installed() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path());
        let http = Http::new().unwrap();
        let release = Release { tag: "v0.0.1".into(), version: version::parse("0.0.1"), ..Release::default() };
        let platform = Platform { os: Os::Windows, arch: Arch::X64 };
        let error = update(&Install::Msi(PathBuf::new()), &release, platform, &http, &paths, &Stop::default(), &mut |_| {}, &|| Ok(())).unwrap_err();
        assert!(error.contains("is not newer than the running version"), "{error}");
    }

    #[test]
    fn a_pretend_version_wins_over_the_package_version() {
        assert_eq!(version_from(Some("0.0.9"), "0.1.1"), version::parse("0.0.9"));
        assert_eq!(version_from(Some("not a version"), "0.1.1"), version::parse("0.1.1"));
        assert_eq!(version_from(None, "0.1.1"), version::parse("0.1.1"));
    }

    #[test]
    fn a_development_build_does_not_update_itself() {
        assert_eq!(current(), None);
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        std::fs::create_dir_all(target.join("release")).unwrap();
        std::fs::write(target.join(".rustc_info.json"), "{}").unwrap();
        assert!(is_development_build(&target.join("release").join("keep-launcher"), false));
        assert!(!is_development_build(&target.join("release").join("keep-launcher"), true));
        let installed = dir.path().join("Programs").join("keep-launcher");
        assert!(!is_development_build(&installed, false));
    }

    #[test]
    fn a_just_installed_release_is_not_offered_again() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path());
        let release = release_0_1_1();
        assert!(!just_updated_to(&paths, &release));
        mark_installed(&paths, &release);
        assert!(just_updated_to(&paths, &release));
        assert!(!just_updated_to(&paths, &Release { tag: "v0.1.2".into(), ..release.clone() }));
        assert_eq!(unmark_unless_scheduled(&paths, Err("no".into())), Err("no".into()));
        assert!(!just_updated_to(&paths, &release));
    }

    #[test]
    fn a_failure_is_reported_once() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path());
        record_failure(&paths, "the MSI said no");
        assert_eq!(take_failure(&paths).as_deref(), Some("the MSI said no"));
        assert_eq!(take_failure(&paths), None);
    }

    #[test]
    fn the_helper_reads_its_arguments_back() {
        let msi = Finish::Msi(PathBuf::from("/c/downloads/keep-launcher-0.1.2-windows-x64.msi"));
        let exe = Path::new("/c/Programs/MoneyKeep/Launcher/keep-launcher.exe");
        assert_eq!(parse_helper_args(&helper_args(42, exe, &msi)), Some((42, exe.to_path_buf(), msi)));
        let portable = Finish::Portable { zip: PathBuf::from("/c/downloads/x.zip"), tmp: PathBuf::from("/c/cache/launcher-update") };
        assert_eq!(parse_helper_args(&helper_args(7, exe, &portable)), Some((7, exe.to_path_buf(), portable)));
        assert_eq!(parse_helper_args(&[FINISH_FLAG.into()]), None);
        assert_eq!(parse_helper_args(&["--other".into(), "1".into(), "x".into(), "msi".into(), "y".into()]), None);
    }

    #[test]
    fn only_the_launchers_own_exe_counts() {
        assert!(is_own_executable(Path::new("C:/Programs/MoneyKeep/Launcher/keep-launcher.exe")));
        assert!(is_own_executable(Path::new("KEEP-LAUNCHER.EXE")));
        assert!(!is_own_executable(Path::new(concat!("artcraft", "-launcher.exe"))));
        assert!(!is_own_executable(Path::new("moneykeep.exe")));
    }

    #[test]
    fn a_flatpak_copy_knows_its_installation_and_origin() {
        let user = "[Application]\nname=com.moneykeep.keep-launcher\n\n[Instance]\ninstance-id=123\napp-path=/home/a/.local/share/flatpak/app/com.moneykeep.keep-launcher/x86_64/master/abc/files\n";
        assert_eq!(parse_flatpak_level(user), Some(FlatpakLevel::User));
        assert_eq!(parse_flatpak_level(&user.replace("/home/a/.local/share/flatpak/", "/var/lib/flatpak/")), Some(FlatpakLevel::System));
        assert_eq!(parse_flatpak_level("[Instance]\napp-path=/somewhere/else\n"), None);
        assert_eq!(parse_flatpak_instance_id(user).as_deref(), Some("123"));
        assert_eq!(parse_flatpak_host_pid("123 4567\n999 1\n", "123"), Some(4567));
        assert_eq!(parse_flatpak_host_pid("999 1\n", "123"), None);
        assert!(is_bundle_origin("keep-launcher-origin"));
        assert!(is_bundle_origin("keep-launcher2-origin"));
        assert!(!is_bundle_origin("flathub"));
        assert!(!is_bundle_origin(concat!("artcraft", "-launcher-origin")));
    }

    #[cfg(unix)]
    #[test]
    fn the_restart_helper_drops_the_pretend_version() {
        let cmd = helper_command(1, &[std::ffi::OsStr::new("/bin/true")]);
        assert!(cmd.get_envs().any(|(k, v)| k == PRETEND_VAR && v.is_none()));
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn a_standalone_binary_is_replaced_from_the_tarball() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("keep-launcher");
        std::fs::write(&exe, b"old").unwrap();
        let tarball = dir.path().join("update.tar.gz");
        {
            let gz = flate2::write::GzEncoder::new(std::fs::File::create(&tarball).unwrap(), flate2::Compression::fast());
            let mut builder = tar::Builder::new(gz);
            let mut header = tar::Header::new_gnu();
            header.set_size(3);
            header.set_mode(0o755);
            header.set_cksum();
            builder.append_data(&mut header, "keep-launcher-0.1.2/bin/keep-launcher", &b"new"[..]).unwrap();
            builder.into_inner().unwrap().finish().unwrap();
        }
        replace_standalone(&tarball, &exe).unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"new");
    }

    /// Nothing in this crate may describe the launcher with another product's identity again
    /// (0.1.0 and 0.1.1 updated "themselves" as ArtCraft Launcher, and kept the version choices in
    /// the upstream launcher's folder).
    #[test]
    fn no_code_names_another_launcher() {
        // craft-update is no longer a dependency (0.1.2): its self-update, app suite and file
        // locations are the upstream launcher's.
        let forbidden = [concat!("craft", "_update"), concat!("artcraft", "-launcher")];
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut stack = vec![src];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let text = std::fs::read_to_string(&path).unwrap();
                    for word in forbidden {
                        assert!(!text.contains(word), "{} mentions {word}", path.display());
                    }
                }
            }
        }
    }
}
