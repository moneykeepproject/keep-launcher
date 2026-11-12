# AGENTS.md: guide for AI agents and contributors

MoneyKeep Launcher installs, updates and launches the **MoneyKeep Apps** ("MoneyKeep – Apps for
Financial Management"). The catalog (`core::catalog`) has five apps: the four MoneyKeep Apps in
`SUITE` (MoneyKeep, CalcKeep, GroupKeep and Form1099Keep) and MoneyKeep Launcher itself
(`LAUNCHER`), described the same way so it can update itself. The catalog can also carry approved
**community apps** made outside MoneyKeep and labelled as such; there are none today. It is
written in **Rust only** (no JavaScript or TypeScript): the desktop
app is native egui/eframe, and the web build is the same Rust compiled to WebAssembly (trunk +
wasm-bindgen). **No Tauri, Electron or webview shells**, ever.

The product name is always **MoneyKeep Launcher** in user-facing text: UI, window titles, About,
tray menu, notifications, installers, release names, docs prose. The suite is the **MoneyKeep Apps**, and its tagline is **Apps for Financial Management**. Machine names stay lowercase: crates and binaries (`keep-launcher`,
`keep-launcher-*`), file names, and the app id `com.moneykeep.keep-launcher`. The
GitHub repo is `moneykeepproject/keep-launcher`, and every file and package is `keep-launcher-*`. The
publisher (Windows installers, macOS signing) is **MoneyKeep Project**.

MoneyKeep Launcher is a fork of ArtCraft Launcher (`storytold/craft-launcher`, MIT OR Apache-2.0).
Keep the upstream copyright lines in `LICENSE-MIT` and `NOTICE`; nothing else of ArtCraft's brand
belongs in this repo. It still builds on code ArtCraft publishes: the egui and winit forks and
`craft-libs` pinned in `Cargo.toml`. Those are dependencies, not branding: leave their names alone.
The CI and release workflows are this repository's own (see "CI workflows" below).

## 0. Clean-room protocol (mandatory, applies to every change)

MoneyKeep Launcher belongs to the MoneyKeep Apps, a clean-room implementation of personal and
small-business finance software. MoneyKeep's full protocol, with its role roster and its list of
prior-art projects, is in `docs/clean-room.md` of the MoneyKeep repository
(`moneykeepproject/moneykeep`). The short version, which you must follow here too:

- **Know your role for the task.** You are either a **spec writer** (may study target
  products and prior art through use and public docs; writes only specs) or an
  **implementer** (writes code from specs and public standards only). Never both for the
  same feature area.
- **Implementers never read** source code of commercial finance products, any
  proprietary product, or any project listed under "Prior art" in MoneyKeep's
  `docs/clean-room.md`, whatever its licence (GPL, AGPL, LGPL, BSL, source-available, MIT
  or BSD).
- **Never** decompile, disassemble or reverse-engineer target binaries or their
  proprietary file formats.
- **Never** copy trademarks of commercial finance products, or their icons, artwork,
  report layouts, help text or UI wording.
- **Never** scrape or automate bank websites (browser automation is out of scope:
  institutions' terms of use prohibit it).
- If you are an AI agent and you are given, or come across, forbidden material: stop,
  do not use it, and say so in your output.

## 1. Workspace map

```text
crates/
  core      keep-launcher-core      pure and wasm-safe: the app catalog, versions, GitHub
                                        release parsing, picking the right asset per platform
  native    keep-launcher-native    desktop only: HTTP, detecting installed apps, download,
                                        verify and install
  ui-egui   keep-launcher-ui-egui   the shared egui UI (desktop and web)
apps/
  keep-launcher                     desktop shell (eframe): window, tray icon, notifications
  keep-launcher-web                 the same UI in the browser (trunk; catalog and downloads only)
xtask/                                  cargo xtask ci | wasm | version | ico | workflows
packaging/                              macOS, Windows, Linux, FreeBSD and web packaging scripts
assets/app-icon/                        the launcher's own icon (master SVG, renders, tray icons)
assets/app-icons/                       the MoneyKeep apps' icons (placeholders until each app has its own)
assets/fonts/                           Archivo, Inter, Geist Mono (OFL-1.1)
```

Layering: `core` depends on no other workspace crate and must build for `wasm32-unknown-unknown`.
`native` may use `core`. `ui-egui` may use `core` (and `native` only behind
`cfg(not(target_arch = "wasm32"))` or through a trait the desktop shell implements). Nothing below
the apps opens windows, tray icons or notifications.

## 2. Golden rules

### Never crash (outranks feature work)

A bad network response, a malformed release, a half-downloaded file, a full disk or a corrupt
settings file must produce an error the user can act on, never a panic.

- **Non-test code never panics.** No `unwrap()`, `expect()`, `panic!`, `unreachable!`, `todo!` or
  `unimplemented!`. Return the crate's error type and propagate with `?`.
- Every production crate root (`lib.rs`, each binary's `main.rs`, and `xtask`) carries
  `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`.
- **No `unsafe`.** The workspace sets `unsafe_code = "forbid"`.
- **Everything from the network is hostile.** Use `get()` instead of indexing, checked arithmetic
  for sizes and offsets, and cap downloads and JSON sizes.

### Installing other people's software safely

- Download only from `https://github.com/moneykeepproject/<app>/releases` (and GitHub's asset CDN), or
  from an approved community app's own repo (there are none today). A community app's repo is pinned in the catalog (`KeepApp::repo`,
  `AppKind::Community`): the release feed can move an app within `moneykeepproject/`, never to another
  owner. The one other exception is a developer's own loopback mirror (`KEEP_LAUNCHER_DOWNLOAD_BASE`,
  `cargo xtask local`); keep it loopback-only and behind the `local-mirror` cargo feature, which
  release builds never enable.
- Verify each download against the release's `SHA256SUMS.txt` before running or installing it.
  Community apps get the same check. On macOS the launcher also requires the Developer ID signature
  of the app's own team (`KeepApp::mac_team`: `catalog::MONEYKEEP_TEAM_ID` for official apps, a community app's own
  team for it); an app with none (`mac_team = None`) is not installed there, only opened if present.
  MoneyKeep has no Developer ID yet, so `MONEYKEEP_TEAM_ID` is `None`: set it once there is one.
- There is no release-feed server (`feed::FEED_URL` is `None`): the launcher asks GitHub directly.
- The details drawer's pictures are the one other download: JPEGs only, from a pinned tag of
  `https://raw.githubusercontent.com/moneykeepproject/keep-launcher-assets/` (`core::details`),
  capped at 2 MB each, cached, and never run. Pictures are not bundled in this repo.
- Never run an installer with elevated rights silently; tell the user what is about to happen.
- Never delete a user's files outside what the launcher itself installed.
- **Agents never land or approve a third-party app.** A change that adds a catalog row, download
  source or install path for an app whose repo is not under `moneykeepproject/` and isn't one of the
  approved community apps above, or that loosens the rules above to allow one, is a maintainer's
  call alone. An agent reviewing it flags it at the
  top of its review (who owns the repo, its age and history, whether the builds are signed) and
  stops there: no merge, no approval, no fixes pushed to the PR.

### Look and feel

The UI keeps the look it was built with: hard square corners, 1 px hairlines instead of shadows, Archivo (620, 118 % width) for headings
and app names, Inter for body text, upper-case Geist Mono labels and buttons, and the colour
tokens by name (`theme::Tokens`). Build new UI from `widgets.rs`; never hard-code a colour.

Animations follow the OS: with Windows' "Animation effects" off, macOS' "Reduce motion" on, a Linux
desktop's animations off or a browser's `prefers-reduced-motion`, nothing fades, wipes or glides
(`Host::reduce_motion`, `crates/ui-egui/src/motion.rs`). Time a new animation with egui's
`style.animation_time`, or switch it off on `LauncherUi::reduce_motion`. Progress bars and
spinners keep moving.

### Languages

Every user-facing string goes through `crates/ui-egui/src/i18n` (`tl!("…")` for literals, `fmt`
for `{placeholders}`, `tn` for counts). English in the code is the lookup key; adding a string
means adding its row to every catalog marked `complete_menus` (the `*.tsv` files in
`crates/ui-egui/src/i18n/`), which the i18n tests check. Translate clean-room from the meaning of
the English text, never through another catalog. Product and app names (MoneyKeep Launcher, MoneyKeep
Apps, MoneyKeep, CalcKeep) stay in English.
A CJK language is drawn with the system's own CJK font (`crates/ui-egui/src/cjk_font.rs`, desktop
only; English where there is none): never bundle a CJK font, they are 10 MB and more. Georgian
borrows the system's Georgian font the same way.

### Other rules

1. **Assets need attribution.** Every bundled asset gets a row in `ATTRIBUTION.md` (path, title,
   author, source, license) in the same change. The MoneyKeep name, logo and calculator mark in
   `docs/brand/` (the header logo and the tray icon) are trademarks of MoneyKeep Project,
   **not MIT/Apache** (`docs/brand/LICENSE-brand.txt`).
2. **Icons are generated.** The app icon and the tray icon are the MoneyKeep calculator mark from
   `docs/brand/moneykeep-mark.svg` (a trademark, not MIT/Apache); never edit the renders by hand. Run `packaging/icons.sh` (needs `resvg`; runs on any OS) and commit the outputs.
   `assets/app-icons/` holds placeholder icons (sources in `assets/app-icons/src/`); once an app
   has its own, copy it verbatim from that app's repo (`assets/app-icon/hicolor/128x128/apps/com.moneykeep.<app>.png`).
3. **Never break wasm.** `cargo xtask wasm` must pass. File-system, process and OS code is
   `cfg(not(target_arch = "wasm32"))`.
4. **Tests are the gate.** Release parsing and asset picking in `core` get unit tests with real
   GitHub release JSON fixtures (small, committed under `crates/core/tests/fixtures/`).
   The real catalog has four MoneyKeep Apps, so tests that need more (a renamed app, a community app, a
   signed macOS build, many apps in different states) use `core::fixtures`: `RENAMED`,
   `COMMUNITY`, `SIGNED`, and `TEST_SUITE`, a seventeen-app catalog the UI tests open with
   `LauncherUi::with_catalog`. Never show fixtures outside tests. `readme_screenshots` (ignored)
   in `crates/ui-egui/tests/ui.rs` redraws `docs/screenshots/` with the real catalog.
5. When fetching anything, use a generic `MoneyKeep-Launcher/<version>` User-Agent and never put
   a person's name, email or other personal details in requests.
6. **Time goes through `jiff`** (workspace dependency). Never hand-roll UTC/calendar conversion
   (days-to-date algorithms, leap-year math); a module owns only its output format string. The one
   exception is range extension: past the years jiff can represent, shift the date by whole 400-year
   Gregorian cycles (146,097 days, after which the calendar repeats), let jiff compute the month and
   day, and add the cycles back to the year (splitting seconds into hours, minutes and seconds is
   plain arithmetic).

## 3. Before you finish a task

```sh
cargo xtask ci              # workflows check, fmt --check, clippy -D warnings, test, wasm check
```

On Windows, `cargo xtask ci` runs the tests with `WGPU_BACKEND=dx12` (the UI test that renders
crashes under Vulkan on some drivers); set `WGPU_BACKEND` yourself to override it, and set it the
same way when you run `cargo test` directly.

Or piecemeal: `cargo test -p <crate>`, `cargo clippy -p <crate> --all-targets -- -D warnings`,
`cargo xtask wasm`. If you touched packaging: `shellcheck packaging/*.sh packaging/*/*.sh
packaging/linux/AppRun` and `actionlint`.

## 4. Releasing

The version lives only in `[workspace.package] version` in the root `Cargo.toml`
(`cargo xtask version`, `cargo xtask version set X.Y.Z[-pre]`). Pushing the tag `v<version>` (it must
match that version) runs `.github/workflows/release.yml`, which builds every platform and creates the
draft GitHub Release **MoneyKeep Launcher v<version>** with `SHA256SUMS.txt`. It can also be run by
hand (workflow_dispatch) with a version override and the `macos` choice described under "CI
workflows". Every package carries `THIRD-PARTY-LICENSES.txt` (every crate compiled into the
program, with its licence and licence texts), written by `cargo xtask licences` through
`packaging/env.sh` (`write_licences`) and `package.ps1`.

Release assets (keep these names exact; the launcher's own self-update and the README rely on them):

| Platform | Files |
|---|---|
| Windows | `keep-launcher-<ver>-windows-{x64,x86,arm64}.msi`, `…-windows-{x64,x86,arm64}-portable.zip` |
| macOS | `keep-launcher-<ver>-macos-universal.dmg` |
| Linux | `keep-launcher-<ver>-linux-{x86_64,aarch64}.{AppImage,AppImage.zsync,deb,rpm,tar.gz,flatpak}`, `keep-launcher-<ver>-linux-riscv64.{deb,rpm,tar.gz}` |
| FreeBSD | `keep-launcher-<ver>-freebsd-x86_64.tar.gz` |
| Web | `keep-launcher-web-<ver>.zip` |

Packaging facts the code relies on:

- The Windows MSI is **per-user** (no UAC prompt), by default into
  `%LOCALAPPDATA%\Programs\MoneyKeep\Launcher`. Double-clicked it opens a WixUI setup wizard
  (Welcome; a folder page with a "Create a desktop shortcut" box; Ready; progress; Finish with
  "Launch"), whose art `package.ps1` draws at build time; self-update runs it with `/passive`, no
  wizard. The folder and the shortcut choice are recorded under
  `HKCU\Software\MoneyKeep Project\MoneyKeep Launcher` (`InstallDir`,
  `DesktopShortcutChoice`) and reused by the next version, and `self_update.rs` reads `InstallDir`
  to know the running copy came from the MSI. Packages before 0.1.0-alpha.1 used the publisher
  name `MoneyKeep Project Inc`: the MSI and `detect.rs` still read that key (and accept that
  publisher on uninstall entries) as a fallback, so an existing install is found on upgrade. Its shortcuts set AppUserModelID
  `com.moneykeep.keep-launcher`, which toast notifications need. `portable.txt` beside the exe switches on portable mode
  (`MoneyKeepLauncherData\` next to the exe).
- The macOS bundle is `MoneyKeep Launcher.app` (executable `Contents/MacOS/MoneyKeep Launcher`), a
  normal Dock app that can also sit in the menu bar: `LSUIElement` must never be set.
- Linux: the tray uses StatusNotifierItem over D-Bus and notifications use
  `org.freedesktop.Notifications`, both in pure Rust (no GTK, appindicator or libdbus).
- Inside the Flatpak launcher (`process::in_sandbox()`), apps normally install as host Flatpaks
  and run through `flatpak-spawn --host`. When the host has no usable Flatpak — none installed,
  or `flatpak-spawn` not allowed — `linux::order_available_kinds` offers the release tarball
  instead: it is unpacked into `<data>/apps/<app>` (`bin/`, `share/`) and run with `HOME` =
  `<data>/apps/<app>/home`, so a MoneyKeep App's settings stay in the sandbox. An update swaps
  `bin/` and `share/` and keeps `home/`; removing the app takes those two folders only. Outside
  a sandbox the tarball is not on offer at all.
- `--socket=pulseaudio` in the launcher manifests is for an app running *inside* the sandbox
  (its ALSA output reaches the runtime's PulseAudio plugin through it); it also covers the
  microphone.
- `build.rs` reads `KEEP_LAUNCHER_BUILD_SHA` / `KEEP_LAUNCHER_BUILD_DATE` and, on Windows,
  embeds `assets/app-icon/keep-launcher.ico`, failing the build when
  `KEEP_LAUNCHER_REQUIRE_WINRES=1` and it can't.

## 5. Parallel agents

- Use your own target dir (`CARGO_TARGET_DIR=target/agent-<name>`) and edit only the files you own.
- Keep every `Cargo.toml` valid at all times: the `crates/*` and `apps/*` globs mean one broken
  manifest breaks everyone's build. Write manifests to a temp file and `mv` them into place.
- If someone else's in-progress edit breaks the build, wait and retry; don't fix their files.

## CI workflows

- Every workflow in `.github/workflows/` is self-contained: its jobs are defined in this
  repository and call no other organisation's reusable workflow or action (security-review-1 M9).
  `ci.yml` (fmt, clippy, tests, wasm, the web package), `packaging-lint.yml`, `freebsd.yml`,
  `nix.yml`, `windows-arm64.yml` and `release.yml`.
- Actions are GitHub's own (`actions/*`), plus `vmactions/freebsd-vm` for the FreeBSD VM and
  `DeterminateSystems/nix-installer-action` for Nix. Each is pinned to the full commit SHA of a
  tag, taken from that action's own repository and named in a comment beside it. Tools
  (nfpm, trunk, actionlint, WiX) are downloaded from their publishers and checked against a
  SHA-256 written in the workflow, or installed with `dotnet tool install` at a fixed version.
- Signing secrets are passed only to the release steps that sign. The Windows ones live in the
  repository's `release` environment (`packaging/windows/sign.ps1`, which never puts a certificate
  password on a command line).
- **macOS releases build on the project's own Mac**, a self-hosted runner (MoneyKeep's
  `docs/specs/macos-release.md`; the owner's setup and upkeep are in MoneyKeep's runbook
  [`docs/deploy/mac-runner.md`](https://github.com/moneykeepproject/moneykeep/blob/main/docs/deploy/mac-runner.md)).
  `release.yml`'s `macos` job runs on `[self-hosted, macOS, ARM64, mk-mac]` and waits for the
  owner's approval in the `mac-release` environment. It checks out clean (`git clean -ffdx`) and
  keeps its build cache in `~/mk-build/keep-launcher`, emptied when `rustc --version` changes. It
  signs with the Developer ID key in the Mac's own `mk-signing.keychain-db` and notarizes with
  `notarytool --keychain-profile mk-notary` (`packaging/macos/runner-keychain.sh`), so no Apple
  secret is stored in GitHub; with no such keychain it signs ad hoc with a warning. A tag push
  always uses the Mac; a hand run chooses `macos: runner` (default), `github` (GitHub's
  `macos-15`, signing from the `APPLE_*` secrets in the `mac-release` environment through
  `import-cert.sh`, for when the Mac is away) or `skip` (no macOS asset; the draft release says
  so).
- **Only `release.yml` may name `self-hosted`**, and only while it runs on a pushed `v*` tag and
  `workflow_dispatch`: strangers' pull requests must never reach the Mac. `cargo xtask workflows`
  (part of `cargo xtask ci`) fails otherwise, and also when a self-hosted job lacks
  `environment: mac-release`. Pull-request testing stays on GitHub's machines (`ci.yml`).
- Don't add or change CI in a feature or fix PR; a CI change is its own PR.
