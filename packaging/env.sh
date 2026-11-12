# shellcheck shell=bash
# Shared setup for the packaging scripts. Source it: `. "$(dirname "$0")/../env.sh"`.
#
# Exports:
#   ROOT                           workspace root
#   VERSION                        [workspace.package] version from Cargo.toml
#                                  (override: KEEP_LAUNCHER_VERSION)
#   DIST                           output directory for release artifacts (default: $ROOT/dist/release)
#   KEEP_LAUNCHER_BUILD_SHA    git commit baked into the binaries (read by the app's build.rs)
#   KEEP_LAUNCHER_BUILD_DATE   UTC build date, YYYY-MM-DD
#   CARGO_TARGET_DIR               cargo's target dir (default: $ROOT/target)

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export ROOT

# The version lives in exactly one place: `[workspace.package] version` in the root Cargo.toml.
# (`cargo xtask version` prints the same thing; awk avoids compiling xtask here.)
workspace_version() {
  awk '
    /^\[/ { in_pkg = ($0 == "[workspace.package]") ; next }
    in_pkg && $1 == "version" { gsub(/[" ]/, "", $3); print $3; exit }
  ' "$ROOT/Cargo.toml"
  return
}

VERSION="${KEEP_LAUNCHER_VERSION:-$(workspace_version)}"
if [[ -z "$VERSION" ]]; then
  echo "error: could not read [workspace.package] version from $ROOT/Cargo.toml" >&2
  exit 1
fi
export VERSION

DIST="${DIST:-$ROOT/dist/release}"
mkdir -p "$DIST"
export DIST

if [[ -z "${KEEP_LAUNCHER_BUILD_SHA:-}" ]]; then
  KEEP_LAUNCHER_BUILD_SHA="$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || true)"
fi
export KEEP_LAUNCHER_BUILD_SHA
export KEEP_LAUNCHER_BUILD_DATE="${KEEP_LAUNCHER_BUILD_DATE:-$(date -u +%Y-%m-%d)}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"

# Emit a GitHub Actions warning (plain stderr outside Actions).
warn() {
  if [[ -n "${GITHUB_ACTIONS:-}" ]]; then echo "::warning::$*"; else echo "warning: $*" >&2; fi
  return
}

# Copy licence, notice and readme files that exist into a package directory.
copy_docs() {
  local dest="$1" f
  for f in README.md LICENSE-MIT LICENSE-APACHE NOTICE ATTRIBUTION.md; do
    if [[ -f "$ROOT/$f" ]]; then cp "$ROOT/$f" "$dest/"; fi
  done
  return
}

# Write THIRD-PARTY-LICENSES.txt into a package directory (security-review-1 M8): every crate
# compiled into the program, with its version, licence and licence texts, from `cargo metadata`
# (cargo xtask licences). $1: the directory; $2: the Rust target the program is built for (empty:
# this computer's), or --web for the browser program. xtask itself is built for this computer, so
# CARGO_BUILD_TARGET (a cross build's) is left out of its environment.
write_licences() {
  local dest="$1" target="${2:-}" args=()
  if [[ "$target" == --web ]]; then
    args=(--web)
  elif [[ -n "$target" ]]; then
    args=(--target "$target")
  fi
  (cd "$ROOT" && env -u CARGO_BUILD_TARGET cargo run --quiet --locked -p xtask -- licences ${args[@]+"${args[@]}"} "$dest/THIRD-PARTY-LICENSES.txt")
  return
}

# Portable SHA-256 of a file (prints just the hash).
sha256() {
  local file="$1"
  if command -v sha256sum >/dev/null; then sha256sum "$file" | cut -d' ' -f1; else shasum -a 256 "$file" | cut -d' ' -f1; fi
  return
}
