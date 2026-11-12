//! The MoneyKeep Apps (Apps for Financial Management): the apps the launcher knows about.
//!
//! The list is data so a later version can fetch it from an API; [`SUITE`] is the built-in copy.
//!
//! Text here is English, the source the UI translates from: each `tagline` has a catalog row
//! keyed by the app id (`tr_ctx(app.id, app.tagline)` in `ui-egui`).

/// Who makes an app. Official apps live under [`GITHUB_OWNER`]; a community app is maintained
/// outside MoneyKeep and is labelled as such wherever it is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppKind {
    Official,
    Community,
}

/// One app in the suite.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeepApp {
    /// Machine name: repo, binary, package and asset stem (`moneykeep`).
    pub id: &'static str,
    /// Display name (`MoneyKeep`).
    pub name: &'static str,
    /// What it's for, in a few words (English; the UI translates it by app id).
    pub tagline: &'static str,
    /// Brand colour (sRGB), for surfaces (cards, badges).
    pub color: [u8; 3],
    /// The brand colour for text on a light background .
    pub ink_light: [u8; 3],
    /// The brand colour for text on a dark background .
    pub ink_dark: [u8; 3],
    /// GitHub `owner/repo`.
    pub repo: &'static str,
    /// Older names its release assets may carry, after a rename.
    pub former_ids: &'static [&'static str],
    /// Its desktop program's file names without `.exe`, current first, when they differ from its
    /// [`KeepApp::stems`] (MoneyKeep's desktop app is `moneykeep-app.exe`; its `moneykeep.exe`
    /// is the command-line tool, which must not be what "Open" runs).
    pub programs: &'static [&'static str],
    /// Older display names its installers registered, after a rename.
    pub former_names: &'static [&'static str],
    pub kind: AppKind,
    /// Who publishes it, as the card's byline shows it (`MoneyKeep`, or a community app's maker).
    pub publisher: &'static str,
    /// The publisher its Windows installer names, when it isn't MoneyKeep's ([`MSI_PUBLISHER`]).
    pub msi_publisher: Option<&'static str>,
    /// The bundle id when it isn't `com.moneykeep.<id>` (a community app brings its own).
    pub bundle: Option<&'static str>,
    /// Older bundle ids its Flatpak shipped under, after a rename.
    pub former_bundles: &'static [&'static str],
    /// Its website, if it has one besides its GitHub repository.
    pub site: Option<&'static str>,
    /// The Apple Developer team whose Developer ID signature its macOS build must carry. `None`
    /// while those builds carry no signature the launcher can check: it then opens a copy that's
    /// there and links to the releases, but installs nothing.
    pub mac_team: Option<&'static str>,
}

impl KeepApp {
    /// `com.moneykeep.<id>`: macOS bundle id, Flatpak id and Linux desktop-file id.
    pub fn bundle_id(&self) -> String {
        self.bundle.map_or_else(|| format!("com.moneykeep.{}", self.id), str::to_string)
    }

    pub fn releases_url(&self) -> String {
        format!("https://github.com/{}/releases", self.repo)
    }

    pub fn repo_url(&self) -> String {
        format!("https://github.com/{}", self.repo)
    }

    /// The app's own website, when it has one (its GitHub repository is [`Self::repo_url`]).
    pub fn website(&self) -> Option<String> {
        self.site.map(str::to_string)
    }

    /// Whether the launcher installs this app from a `kind` package.
    pub fn installs(&self, kind: crate::PackageKind) -> bool {
        kind != crate::PackageKind::Dmg || self.mac_team.is_some()
    }

    /// Bundle ids this app may be installed under, current first.
    pub fn bundle_ids(&self) -> impl Iterator<Item = String> + '_ {
        std::iter::once(self.bundle_id()).chain(self.former_bundles.iter().map(|b| b.to_string()))
    }

    /// Asset stems this app's releases may use, current first.
    pub fn stems(&self) -> impl Iterator<Item = &'static str> + '_ {
        std::iter::once(self.id).chain(self.former_ids.iter().copied())
    }

    /// The desktop program's possible file names without `.exe`, current first: [`KeepApp::programs`]
    /// when set, else the [`KeepApp::stems`].
    pub fn exe_names(&self) -> Vec<&'static str> {
        if self.programs.is_empty() { self.stems().collect() } else { self.programs.to_vec() }
    }

    /// Display names this app's installers may have registered, current first.
    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        std::iter::once(self.name).chain(self.former_names.iter().copied())
    }

    pub fn by_id(id: &str) -> Option<&'static KeepApp> {
        SUITE.iter().find(|a| a.id == id)
    }
}

const fn app(id: &'static str, name: &'static str, tagline: &'static str, [color, ink_light, ink_dark]: [[u8; 3]; 3], repo: &'static str) -> KeepApp {
    KeepApp {
        id,
        name,
        tagline,
        color,
        ink_light,
        ink_dark,
        repo,
        former_ids: &[],
        programs: &[],
        former_names: &[],
        kind: AppKind::Official,
        publisher: "MoneyKeep",
        msi_publisher: None,
        bundle: None,
        former_bundles: &[],
        site: None,
        mac_team: MONEYKEEP_TEAM_ID,
    }
}

/// The MoneyKeep Apps, in display order, then any community apps.
pub const SUITE: [KeepApp; 4] = [
    KeepApp {
        programs: &["moneykeep-app"],
        ..app(
            "moneykeep",
            "MoneyKeep",
            "Personal and small-business accounting",
            [[0x12, 0xa1, 0x50], [0x0b, 0x6e, 0x36], [0x4f, 0xd6, 0x8a]],
            "moneykeepproject/moneykeep",
        )
    },
    app("calckeep", "CalcKeep", "Financial calculators", [[0x1e, 0xc6, 0xa0], [0x0a, 0x7a, 0x60], [0x4f, 0xe3, 0xbd]], "moneykeepproject/calckeep"),
    app(
        "groupkeep",
        "GroupKeep",
        "Dividing group expenses",
        [[0xe0, 0x70, 0x2d], [0xa8, 0x4a, 0x12], [0xff, 0x9d, 0x5e]],
        "moneykeepproject/groupkeep",
    ),
    app(
        "form1099keep",
        "Form1099Keep",
        "Forms 1099-NEC and 1099-MISC",
        [[0x0f, 0x5e, 0x5c], [0x0f, 0x5e, 0x5c], [0x5f, 0xc9, 0xc4]],
        "moneykeepproject/form1099keep",
    ),
];

/// The publisher name every official MoneyKeep App's Windows installer (and the launcher's own)
/// carries: the Windows side of the macOS signing check.
pub const MSI_PUBLISHER: &str = "MoneyKeep Project";

/// The Apple Developer team that signs every official MoneyKeep App and MoneyKeep Launcher.
/// `None` until MoneyKeep Project has a Developer ID: until then the launcher opens a copy
/// that's already in Applications and links to the releases, but installs nothing on macOS.
pub const MONEYKEEP_TEAM_ID: Option<&str> = None;

/// MoneyKeep's website. The GitHub organisation until MoneyKeep has a site of its own.
pub const WEBSITE_URL: &str = "https://github.com/moneykeepproject";
/// The GitHub account every official MoneyKeep App's repository lives under. The release feed
/// can only move an app within it; a community app installs from its own repo, pinned in [`SUITE`].
pub const GITHUB_OWNER: &str = "moneykeepproject";
/// MoneyKeep on GitHub: every MoneyKeep App's source.
pub const GITHUB_URL: &str = "https://github.com/moneykeepproject";
/// MoneyKeep's social accounts: (service, URL). Shown as small icons. None yet.
pub const SOCIAL: [(&str, &str); 0] = [];

/// MoneyKeep Launcher itself, described like a MoneyKeep App so it can update itself with the
/// same machinery (asset names `keep-launcher-<ver>-…`, bundle id `com.moneykeep.keep-launcher`,
/// `MoneyKeep Launcher.app`). Not part of [`SUITE`].
pub const LAUNCHER: KeepApp = app(
    "keep-launcher",
    "MoneyKeep Launcher",
    "Installs and updates the MoneyKeep Apps",
    [[0x0b, 0x0b, 0x0c], [0x0b, 0x0b, 0x0c], [0xf2, 0xf1, 0xee]],
    "moneykeepproject/keep-launcher",
);

/// The launcher's own repo, checked so it can say when a newer launcher is out.
pub const LAUNCHER_REPO: &str = "moneykeepproject/keep-launcher";
/// The launcher's own asset stem.
pub const LAUNCHER_ID: &str = "keep-launcher";

/// A MoneyKeep App's 128 px icon (PNG), as copied from its repo into `assets/app-icons`: the UI
/// draws it, and the Linux install puts it in the desktop menu.
pub fn app_icon_png(id: &str) -> Option<&'static [u8]> {
    Some(match id {
        "moneykeep" => include_bytes!("../../../assets/app-icons/moneykeep.png"),
        "calckeep" => include_bytes!("../../../assets/app-icons/calckeep.png"),
        "groupkeep" => include_bytes!("../../../assets/app-icons/groupkeep.png"),
        "form1099keep" => include_bytes!("../../../assets/app-icons/form1099keep.png"),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_app_has_its_icon() {
        for app in &SUITE {
            let png = app_icon_png(app.id).unwrap_or_else(|| panic!("no icon for {}", app.id));
            assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"), "{} icon isn't a PNG", app.id);
        }
        assert!(app_icon_png("notanapp").is_none());
    }

    /// Every catalog entry must have a distinct lookup key.
    #[test]
    fn open_runs_the_desktop_program() {
        // MoneyKeep's moneykeep.exe is its command line; "Open" must find moneykeep-app.exe.
        assert_eq!(KeepApp::by_id("moneykeep").unwrap().exe_names(), ["moneykeep-app"]);
        for id in ["calckeep", "groupkeep", "form1099keep"] {
            assert_eq!(KeepApp::by_id(id).unwrap().exe_names(), [id]);
        }
    }

    #[test]
    fn suite_has_unique_ids() {
        assert_eq!(SUITE.len(), 4);
        let mut ids: Vec<_> = SUITE.iter().map(|a| a.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), SUITE.len());
        for a in &SUITE {
            assert_eq!(a.name.to_lowercase().replace(' ', "-"), a.id);
            match a.kind {
                AppKind::Official => assert_eq!(a.repo, format!("{GITHUB_OWNER}/{}", a.id)),
                AppKind::Community => assert!(!a.repo.starts_with("moneykeepproject/"), "{} is community", a.id),
            }
        }
    }

    #[test]
    fn official_apps_use_moneykeep_ids_and_their_repo_as_website() {
        let app = KeepApp::by_id("calckeep").unwrap();
        assert_eq!(app.kind, AppKind::Official);
        assert_eq!(app.bundle_id(), "com.moneykeep.calckeep");
        assert_eq!(app.website(), None);
        assert_eq!(app.releases_url(), "https://github.com/moneykeepproject/calckeep/releases");
        assert_eq!(LAUNCHER.bundle_id(), "com.moneykeep.keep-launcher");
    }

    #[test]
    fn nothing_installs_from_a_dmg_until_there_is_a_signing_team() {
        for app in &SUITE {
            assert_eq!(app.installs(crate::PackageKind::Dmg), MONEYKEEP_TEAM_ID.is_some());
            assert!(app.installs(crate::PackageKind::Msi));
        }
    }
}
