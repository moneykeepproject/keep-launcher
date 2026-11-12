//! Reading system package lists (dpkg, rpm, pacman, Flatpak) and matching them to MoneyKeep Apps.
//!
//! Pure functions over the tools' text output, so they're tested on every platform; only Linux
//! runs the tools (`detect::linux`).

use keep_launcher_core::{InstallSource, KeepApp, version};

/// One installed package that belongs to a MoneyKeep App.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Package {
    pub manager: InstallSource,
    /// The package's name (`moneykeep-bin`) or Flatpak app id.
    pub name: String,
    /// As the package manager reports it (`0.5.0-1`); empty when it doesn't say.
    pub version: String,
    /// A per-user Flatpak install (`flatpak --user`), which the launcher installs and updates.
    pub user: bool,
    /// The origin remote (`flatpark`, `<app>-origin`, `flathub`), when known.
    pub origin: Option<String>,
    /// The installation scope or name (`user`, `system`, or a named installation), for Flatpak installs.
    pub installation: Option<String>,
}

/// `dpkg-query -W -f '${Package}|${db:Status-Status}|${Version}\n'`: installed packages only.
pub fn parse_dpkg(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let mut f = line.split('|');
            match (f.next(), f.next(), f.next()) {
                (Some(name), Some("installed"), Some(ver)) => {
                    let name = name.split_once(':').map(|(n, _)| n).unwrap_or(name);
                    Some((name.to_string(), ver.to_string()))
                }
                _ => None,
            }
        })
        .collect()
}

/// `rpm -qa --qf '%{NAME}|%{VERSION}\n'`.
pub fn parse_rpm(text: &str) -> Vec<(String, String)> {
    text.lines().filter_map(|line| line.split_once('|').map(|(n, v)| (n.to_string(), v.to_string()))).collect()
}

/// `pacman -Q`: `name version` per line.
pub fn parse_pacman(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let mut f = line.split_whitespace();
            Some((f.next()?.to_string(), f.next()?.to_string()))
        })
        .collect()
}

/// `flatpak list --app --columns=application,version,origin,installation`: parses id, version, origin, and installation.
pub fn parse_flatpak_list_with_origin(text: &str) -> Vec<(String, String, Option<String>, Option<String>)> {
    text.lines()
        .filter_map(|line| {
            let mut f = line.split('\t');
            let id = f.next()?.trim();
            if id.is_empty() {
                return None;
            }
            let version = f.next().unwrap_or("").trim().to_string();
            let origin = f.next().map(str::trim).filter(|o| !o.is_empty()).map(ToString::to_string);
            // A named installation lists as `system (<name>)`, but `--installation=` takes the bare name.
            let installation = f
                .next()
                .map(str::trim)
                .filter(|i| !i.is_empty())
                .map(|i| i.strip_prefix("system (").and_then(|n| n.strip_suffix(')')).unwrap_or(i).to_string());
            Some((id.to_string(), version, origin, installation))
        })
        .collect()
}

/// `flatpak list --app --columns=application,version`: the version column is often empty for
/// bundles built from a release tarball.
pub fn parse_flatpak_list(text: &str) -> Vec<(String, String)> {
    parse_flatpak_list_with_origin(text).into_iter().map(|(id, v, _, _)| (id, v)).collect()
}

/// Whether an origin string indicates an install from FlatPark.
pub fn is_flatpark_origin(origin: &str) -> bool {
    origin.trim().eq_ignore_ascii_case("flatpark")
}

/// The `flatpak` argument that selects an installation (`user`, `system`, or a named one).
pub fn flatpak_scope(installation: Option<&str>) -> String {
    match installation {
        Some("user") => "--user".into(),
        Some("system") | None => "--system".into(),
        Some(name) => format!("--installation={name}"),
    }
}

/// The `Version:` line of `flatpak info <id>`.
pub fn parse_flatpak_info(text: &str) -> Option<String> {
    text.lines().find_map(|l| l.trim().strip_prefix("Version:").map(|v| v.trim().to_string())).filter(|v| !v.is_empty())
}

/// The package names a MoneyKeep App may be installed under with `manager`. On Arch the AUR carries
/// each app three ways: built from source (`moneykeep`), the release binary (`moneykeep-bin`)
/// and the latest commit (`moneykeep-git`). Debian and RPM packages may use either the short stem
/// (`moneykeep`) or the reverse-DNS bundle id (`com.moneykeep.moneykeep`).
pub fn candidate_names(app: &KeepApp, manager: InstallSource) -> Vec<String> {
    match manager {
        InstallSource::Flatpak => app.bundle_ids().collect(),
        InstallSource::Pacman => {
            app.stems().flat_map(|s| [s.to_string(), format!("{s}-bin"), format!("{s}-git")]).chain(std::iter::once(app.bundle_id())).collect()
        }
        InstallSource::Deb | InstallSource::Rpm => app.stems().map(str::to_string).chain(std::iter::once(app.bundle_id())).collect(),
        _ => app.stems().map(str::to_string).collect(),
    }
}

/// The package of `app` among `installed`, preferring the manager listed first in `order`. A user
/// install beats a system one under any of the app's ids (#881); within a scope the current id wins.
pub fn find<'a>(app: &KeepApp, installed: &'a [Package], order: &[InstallSource]) -> Option<&'a Package> {
    order.iter().find_map(|manager| {
        let names = candidate_names(app, *manager);
        [true, false]
            .into_iter()
            .find_map(|user| names.iter().find_map(|n| installed.iter().find(|p| p.manager == *manager && p.user == user && p.name == *n)))
    })
}

/// A package's version, read the package-manager way (`0.5.0-1` is 0.5.0).
pub fn version_of(p: &Package) -> Option<version::Version> {
    version::parse_package(&p.version)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkgs(manager: InstallSource, list: Vec<(String, String)>) -> Vec<Package> {
        list.into_iter().map(|(name, version)| Package { manager, name, version, user: false, origin: None, installation: None }).collect()
    }

    #[test]
    fn reads_each_package_managers_listing() {
        assert_eq!(
            parse_dpkg("moneykeep|installed|0.5.0-1\nmoneykeep:amd64|installed|0.6.0-1\ncalckeep|config-files|0.3.0-1\n"),
            [("moneykeep".into(), "0.5.0-1".into()), ("moneykeep".into(), "0.6.0-1".into())]
        );
        assert_eq!(parse_rpm("calckeep|0.3.0\n"), [("calckeep".into(), "0.3.0".into())]);
        assert_eq!(parse_pacman("moneykeep-bin 0.5.0-1\nlinux 6.11.1.arch1-1\n").len(), 2);
        assert_eq!(
            parse_flatpak_list("com.moneykeep.calckeep\t\ncom.moneykeep.assetkeep\t0.3.0\n"),
            [("com.moneykeep.calckeep".into(), String::new()), ("com.moneykeep.assetkeep".into(), "0.3.0".into())]
        );
        let info = "\nCalcKeep - Word processor\n\n          ID: com.moneykeep.calckeep\n     Version: 0.3.0\n      Branch: master\n";
        assert_eq!(parse_flatpak_info(info).as_deref(), Some("0.3.0"));
        assert_eq!(parse_flatpak_info("Version: \n"), None);
    }

    #[test]
    fn taxkeep_flatpak_is_found_under_its_older_ids_current_first() {
        let sc = &keep_launcher_core::fixtures::COMMUNITY;
        let old = pkgs(InstallSource::Flatpak, parse_flatpak_list("com.moneykeep.oldtaxkeep\t0.7.0\n"));
        assert_eq!(find(sc, &old, &[InstallSource::Flatpak]).map(|p| p.name.as_str()), Some("com.moneykeep.oldtaxkeep"));
        let both = pkgs(InstallSource::Flatpak, parse_flatpak_list("com.moneykeep.taxkeep\t0.8.0\norg.example.taxkeep\t0.25.1\n"));
        assert_eq!(find(sc, &both, &[InstallSource::Flatpak]).map(|p| p.name.as_str()), Some("org.example.taxkeep"));
        // Another app's old id never matches TaxKeep.
        let other = pkgs(InstallSource::Flatpak, parse_flatpak_list("com.moneykeep.assetkeep\t0.3.0\n"));
        assert!(find(sc, &other, &[InstallSource::Flatpak]).is_none());
    }

    #[test]
    fn a_user_flatpak_beats_a_system_one_under_any_id() {
        let sc = &keep_launcher_core::fixtures::COMMUNITY;
        let mut installed = pkgs(InstallSource::Flatpak, parse_flatpak_list("org.example.taxkeep\t0.25.1\n"));
        installed.push(Package {
            manager: InstallSource::Flatpak,
            name: "com.moneykeep.taxkeep".into(),
            version: "0.26.0".into(),
            user: true,
            origin: None,
            installation: None,
        });
        assert_eq!(find(sc, &installed, &[InstallSource::Flatpak]).map(|p| p.name.as_str()), Some("com.moneykeep.taxkeep"));
        // Within the user scope the current id still wins.
        installed.push(Package {
            manager: InstallSource::Flatpak,
            name: "org.example.taxkeep".into(),
            version: "0.26.1".into(),
            user: true,
            origin: None,
            installation: None,
        });
        assert_eq!(find(sc, &installed, &[InstallSource::Flatpak]).map(|p| (p.name.as_str(), p.user)), Some(("org.example.taxkeep", true)));
    }

    #[test]
    fn candidate_names_cover_bundle_ids_for_deb_rpm_and_pacman() {
        let photo = KeepApp::by_id("moneykeep").unwrap();
        let deb_names = candidate_names(photo, InstallSource::Deb);
        assert!(deb_names.contains(&"moneykeep".to_string()));
        assert!(deb_names.contains(&"com.moneykeep.moneykeep".to_string()));

        let rpm_names = candidate_names(photo, InstallSource::Rpm);
        assert!(rpm_names.contains(&"moneykeep".to_string()));
        assert!(rpm_names.contains(&"com.moneykeep.moneykeep".to_string()));

        let pacman_names = candidate_names(photo, InstallSource::Pacman);
        assert!(pacman_names.contains(&"moneykeep".to_string()));
        assert!(pacman_names.contains(&"moneykeep-bin".to_string()));
        assert!(pacman_names.contains(&"com.moneykeep.moneykeep".to_string()));
    }

    #[test]
    fn finds_aur_packages_in_every_flavour() {
        let photo = KeepApp::by_id("moneykeep").unwrap();
        let pdf = &keep_launcher_core::fixtures::RENAMED;
        let installed = pkgs(InstallSource::Pacman, parse_pacman("linux 6.11.1.arch1-1\nmoneykeep-git 0.3.0.r47.g47f9306-1\nbookkeep-bin 0.2.1-1\n"));
        let hit = find(photo, &installed, &[InstallSource::Pacman]).unwrap();
        assert_eq!(hit.name, "moneykeep-git");
        assert_eq!(version_of(hit).unwrap().to_string(), "0.3.0");
        // LedgerKeep under its old name.
        assert_eq!(find(pdf, &installed, &[InstallSource::Pacman]).unwrap().name, "bookkeep-bin");
        assert!(find(KeepApp::by_id("calckeep").unwrap(), &installed, &[InstallSource::Pacman]).is_none());
    }

    #[test]
    fn prefers_managers_in_order_and_reads_revisions() {
        let photo = KeepApp::by_id("moneykeep").unwrap();
        let mut installed = pkgs(InstallSource::Deb, parse_dpkg("moneykeep|installed|0.5.0-1\n"));
        installed.extend(pkgs(InstallSource::Flatpak, parse_flatpak_list("com.moneykeep.moneykeep\t0.4.0\n")));
        let hit = find(photo, &installed, &[InstallSource::Deb, InstallSource::Flatpak]).unwrap();
        assert_eq!(hit.manager, InstallSource::Deb);
        // 0.5.0-1 is the 0.5.0 release, not a pre-release of it.
        assert_eq!(version_of(hit).unwrap().to_string(), "0.5.0");
        assert_eq!(find(photo, &installed, &[InstallSource::Flatpak]).unwrap().version, "0.4.0");
    }

    #[test]
    fn parses_flatpak_list_with_origin_and_spots_flatpark() {
        let text = "com.moneykeep.moneykeep\t0.19.0\tflatpark\tuser\ncom.moneykeep.assetkeep\t0.16.0\tassetkeep-origin\tsystem\ncom.moneykeep.calckeep\t0.14.0\t\t\ncom.moneykeep.billkeep\t0.3.0\tflatpark\tsystem (extra)\n";
        let list = parse_flatpak_list_with_origin(text);
        assert_eq!(
            list,
            [
                ("com.moneykeep.moneykeep".into(), "0.19.0".into(), Some("flatpark".into()), Some("user".into())),
                ("com.moneykeep.assetkeep".into(), "0.16.0".into(), Some("assetkeep-origin".into()), Some("system".into())),
                ("com.moneykeep.calckeep".into(), "0.14.0".into(), None, None),
                ("com.moneykeep.billkeep".into(), "0.3.0".into(), Some("flatpark".into()), Some("extra".into())),
            ]
        );
        assert_eq!(flatpak_scope(Some("extra")), "--installation=extra");
        assert_eq!(flatpak_scope(Some("user")), "--user");
        assert_eq!(flatpak_scope(None), "--system");
        assert!(is_flatpark_origin("flatpark"));
        assert!(is_flatpark_origin("FlatPark"));
        assert!(!is_flatpark_origin("flatpark-origin"));
        assert!(!is_flatpark_origin("assetkeep-origin"));
        assert!(!is_flatpark_origin("flathub"));
        assert!(!is_flatpark_origin(""));
    }
}

/// Placeholder: MoneyKeep has no OCI Flatpak remote yet, so `moneykeep.example` (a reserved,
/// never-resolving domain) stands in for its address and nothing matches until one exists.
///
/// Matches the OCI URL from https://moneykeep.example/moneykeep-oci.flatpakrepo,
/// independently of its local remote name (normally `moneykeep-oci`).
pub fn is_moneykeep_remote(origin: &str, remotes: &str) -> bool {
    remotes.lines().any(|line| {
        let Some((name, url)) = line.split_once('\t') else { return false };
        let url = url.trim().trim_end_matches('/');
        name.trim() == origin && url == "oci+https://moneykeep.example"
    })
}

/// Where Flatpak finds the MoneyKeep OCI images' signatures (moneykeep-oci.flatpakrepo's
/// `SignatureLookaside`), redirected to each release's `.oci.sig` asset.
pub const MONEYKEEP_SIGNATURE_LOOKASIDE: &str = "https://moneykeep.example/signatures";

/// Did an update fail only because `origin`, an MoneyKeep OCI remote, has no signature lookaside
/// (a signed bundle's origin, which `flatpak build-bundle` can't give one)?
pub fn needs_signature_lookaside(stderr: &str, origin: &str, remotes: &str) -> bool {
    stderr.contains(&format!("{origin}: no valid signatures for ")) && is_moneykeep_remote(origin, remotes)
}

#[cfg(test)]
mod remote_url_tests {
    use super::*;
    #[test]
    fn recognizes_custom_names_and_rejects_unrelated_urls() {
        assert!(is_moneykeep_remote("moneykeep-oci", "moneykeep-oci\toci+https://moneykeep.example\n"));
        assert!(is_moneykeep_remote("custom", "custom\toci+https://moneykeep.example/\n"));
        for url in [
            "oci+https://moneykeep.example.evil",
            "oci+https://moneykeep.example/other",
            "oci+http://moneykeep.example",
            "https://moneykeep.example/linux",
            "https://moneykeep.example/moneykeep-oci.flatpakrepo",
            "https://moneykeep.example",
        ] {
            assert!(!is_moneykeep_remote("custom", &format!("custom\t{url}")));
        }
        assert!(!is_moneykeep_remote("other", "custom\toci+https://moneykeep.example"));
    }

    #[test]
    fn only_an_moneykeep_origin_missing_its_signatures_gets_the_lookaside() {
        // Flatpak 1.18's error for a verified OCI remote with no lookaside.
        let stderr = "error: Failed to update com.moneykeep.moneykeep: moneykeep-origin: no valid signatures for \
                      ghcr.io/moneykeepproject/moneykeep@sha256:968d75c9d16d5f7d089775f14a3b418d72a4c70d86986811ee2dd818da956b92";
        let remotes = "moneykeep-origin\toci+https://moneykeep.example\nflathub\thttps://dl.flathub.org/repo/\n";
        assert!(needs_signature_lookaside(stderr, "moneykeep-origin", remotes));
        // Another remote, another origin's failure, or another error: leave the remote alone.
        assert!(!needs_signature_lookaside(stderr, "moneykeep-origin", "moneykeep-origin\thttps://example.org\n"));
        assert!(!needs_signature_lookaside(stderr, "flathub", remotes));
        assert!(!needs_signature_lookaside("error: Server returned status 404", "moneykeep-origin", remotes));
    }
}

/// A bundle origin permits replacement when its URL is empty or points at MoneyKeep OCI.
/// Missing rows are unknown, so preserve remote ownership conservatively.
pub fn remote_manages_flatpak(origin: &str, remotes: &str) -> bool {
    if origin.ends_with("-origin") && is_moneykeep_remote(origin, remotes) {
        return false;
    }
    remotes
        .lines()
        .find_map(|line| {
            let mut fields = line.split('\t');
            if fields.next().map(str::trim) != Some(origin) {
                return None;
            }
            Some(fields.next().is_some_and(|url| !url.trim().is_empty()))
        })
        .unwrap_or(true)
}

#[cfg(test)]
mod remote_ownership_tests {
    use super::*;
    #[test]
    fn only_confirmed_empty_urls_allow_bundle_replacement() {
        assert!(!remote_manages_flatpak("app-origin", "app-origin\t\n"));
        assert!(remote_manages_flatpak("renamed", "renamed\thttps://example.org/repo\n"));
        assert!(remote_manages_flatpak("renamed", "renamed\toci+https://moneykeep.example\n"));
        assert!(!remote_manages_flatpak("app-origin", "app-origin\toci+https://moneykeep.example\n"));
        assert!(remote_manages_flatpak("missing", "other\t\n"));
        assert!(!remote_manages_flatpak("app-origin", "app-origin"));
    }
}
