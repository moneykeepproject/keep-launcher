# MoneyKeep Launcher app icon

The app icon is the **MoneyKeep calculator badge on dark green**, the same as the MoneyKeep
website's icon, from [`docs/brand/moneykeep-badge.svg`](../../docs/brand/moneykeep-badge.svg).
Every file in this directory is rendered from it by `packaging/icons.sh`.

**These files are a MoneyKeep trademark, not open source and not covered by MoneyKeep Launcher's
MIT or Apache-2.0 license.** Their terms are in
[`docs/brand/LICENSE-brand.txt`](../../docs/brand/LICENSE-brand.txt).

## Files

- `keep-launcher.svg`: a copy of `docs/brand/moneykeep-badge.svg`; also the hicolor scalable icon.
- `keep-launcher-1024.png`: 1024 px render, with extra transparent margin for Apple's icon grid.
- `keep-launcher.icns`: macOS bundle icon (`CFBundleIconFile`).
- `keep-launcher.ico`: Windows icon, 16–256 px, embedded in the `.exe` by
  `apps/keep-launcher/build.rs` and used by the MSI.
- `hicolor/<size>/apps/com.moneykeep.keep-launcher.png` (16–512) and
  `hicolor/scalable/apps/com.moneykeep.keep-launcher.svg`: Linux icon theme.
- The tray / menu-bar icon is the same mark, rendered into `docs/brand/moneykeep-mark-black-44.png`
  (macOS template, 22 pt @2x) and `docs/brand/moneykeep-mark-64.png` (green; Windows, Linux, FreeBSD).

## Regenerate

Run `packaging/icons.sh` on any OS. It needs `resvg`; the `.ico` and `.icns` are packed by
`cargo xtask ico` / `cargo xtask icns`. Never edit these files by hand: change
`docs/brand/moneykeep-badge.svg` (tray: `moneykeep-mark.svg` and `moneykeep-mark-black.svg`) and run the script again.
