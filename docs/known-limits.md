# Known limits

What MoneyKeep Launcher 0.1.0 doesn't do yet. This list is updated with each release.
For what it does do, see the [release notes](release-notes/0.1.0.md).

## Early software

- **MoneyKeep Launcher is new and in early alpha.** Expect rough edges. If something
  goes wrong, please file an issue with your system, the app you were installing and
  the launcher's log file (`logs/keep-launcher.log` in its data folder; the
  [README](../README.md#where-apps-are-installed) says where that is).
- **The MoneyKeep Apps are early software too.** Keep backups of anything you put in
  them.

## On a Mac

- **The launcher's Mac build is not signed with an Apple Developer ID yet** (it is
  signed "ad hoc"). macOS warns the first time you open it: right-click it, then
  choose **Open** (or System Settings > Privacy & Security > **Open Anyway**).
- **The launcher can't install or update the MoneyKeep Apps on a Mac yet.** It installs
  only Mac builds signed with the project's Developer ID, and there is none yet.
  Until then, install each app from its own release's `.dmg`; the launcher opens the
  copy you installed and links to the release when a new version is out.

## On Windows

- **An app installed for your account only** keeps that folder when the launcher
  updates it. To move it to the folder for everyone, uninstall it and install it again.
- **Close an app before updating it.** A running app is never replaced.

## On Linux and FreeBSD

- **The Flatpak build can reach your computer only as far as Flatpak allows.** The
  AppImage, `.deb` and `.rpm` builds are the recommended way to run the launcher on
  Linux.
- **The tray icon on GNOME** needs the AppIndicator extension.
- **Linux builds need glibc 2.35 or newer** (for example Ubuntu 22.04, Debian 12,
  Fedora 36 or later); the RISC-V builds need glibc 2.39 or newer.
- **Running with Nix is experimental.** Outside NixOS the window may fail to open;
  use the AppImage, `.deb` or `.rpm` instead, or run it through nixGL.

## Everywhere

- **The web version only lists the apps and their downloads.** It can't install,
  update or open anything.
- **The launcher talks to GitHub directly** to find releases, so it needs to reach
  github.com.
- **Languages:** Portuguese from Portugal uses the Brazilian Portuguese text for now.
  Chinese, Japanese and Georgian use your system's own font; where there is none, and
  in the web version, the interface stays in English. App names and release notes are
  shown as published, and text without a translation is shown in English.
- **No community apps yet.** The launcher can list approved apps made outside
  MoneyKeep, but there are none today.
