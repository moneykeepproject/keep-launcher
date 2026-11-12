#!/usr/bin/env bash
# The project's own Mac only (the self-hosted runner in release.yml's macos job): sign and notarize
# with the keychain the owner set up once on that Mac (MoneyKeep's docs/deploy/mac-runner.md, step
# 8), so no Apple secret is stored in GitHub. import-cert.sh is the GitHub-hosted equivalent.
#
#   ~/Library/Keychains/mk-signing.keychain-db   the Developer ID Application certificate and key
#   notarytool profile "mk-notary"               stored in that keychain with store-credentials
#
# Outputs: MACOS_SIGN_IDENTITY (SHA-1 of the identity), MACOS_KEYCHAIN and MACOS_NOTARY_PROFILE,
# appended to $GITHUB_ENV when running in Actions, otherwise printed.
#
# No keychain, or no Developer ID identity in it (MoneyKeep has none yet, owner decision O1): prints
# a warning and exits 0, so package.sh signs ad hoc. An identity whose key codesign can't use (the
# keychain is locked, for example after a restart), or with no working mk-notary profile, is an
# error: a release must not quietly lose its signature or notarization.
# Nothing here reads, asks for or prints a password.
set -euo pipefail
umask 077
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"

KEYCHAIN="${MK_SIGNING_KEYCHAIN:-$HOME/Library/Keychains/mk-signing.keychain-db}"
PROFILE="${MK_NOTARY_PROFILE:-mk-notary}"
RUNBOOK="MoneyKeep's docs/deploy/mac-runner.md"

fail() {
  if [[ -n "${GITHUB_ACTIONS:-}" ]]; then echo "::error::$*"; else echo "error: $*" >&2; fi
  exit 1
}

if [[ ! -f "$KEYCHAIN" ]]; then
  warn "no signing keychain at $KEYCHAIN: macOS build will be signed ad hoc and not notarized (until step 8 of $RUNBOOK is done)"
  exit 0
fi
IDENTITY="$(security find-identity -v -p codesigning "$KEYCHAIN" | awk '/Developer ID Application/ { print $2; exit }')"
if [[ -z "$IDENTITY" ]]; then
  warn "no 'Developer ID Application' identity in $KEYCHAIN: macOS build will be signed ad hoc and not notarized"
  exit 0
fi

# Can codesign use the key without a prompt? Sign a throwaway copy of a system program.
PROBE="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/mk-sign-probe.XXXXXXXX")"
trap 'rm -rf "$PROBE"' EXIT
cp /usr/bin/true "$PROBE/probe"
if ! codesign --force --sign "$IDENTITY" --keychain "$KEYCHAIN" --timestamp=none "$PROBE/probe"; then
  fail "codesign cannot use the key in $KEYCHAIN. If the Mac restarted, unlock the keychain once as mkbuild ($RUNBOOK, step 8), or run the release by hand with macos: github or skip."
fi
# Is the notarization profile there and accepted by Apple? (Lists past submissions; no secret is shown.)
if ! xcrun notarytool history --keychain-profile "$PROFILE" --keychain "$KEYCHAIN" >/dev/null; then
  fail "notarytool profile '$PROFILE' in $KEYCHAIN is missing or not accepted. Store it again ($RUNBOOK, step 8), or run the release by hand with macos: github or skip."
fi

echo "Signing identity $IDENTITY from $KEYCHAIN; notarizing with profile $PROFILE"
if [[ -n "${GITHUB_ENV:-}" ]]; then
  {
    echo "MACOS_SIGN_IDENTITY=$IDENTITY"
    echo "MACOS_KEYCHAIN=$KEYCHAIN"
    echo "MACOS_NOTARY_PROFILE=$PROFILE"
  } >>"$GITHUB_ENV"
else
  echo "MACOS_SIGN_IDENTITY=$IDENTITY"
  echo "MACOS_KEYCHAIN=$KEYCHAIN"
  echo "MACOS_NOTARY_PROFILE=$PROFILE"
fi
