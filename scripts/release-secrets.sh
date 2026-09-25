#!/usr/bin/env bash
# Sets the macOS signing and notarization secrets that .github/workflows/release.yml uses, in
# the repository's `release` environment. Run it yourself, in a terminal (it asks for
# passwords): `sh scripts/release-secrets.sh`. Nothing is written to disk or echoed; every
# value goes to `gh secret set` on stdin. Needs `gh` signed in as a repository admin, and
# openssl. Re-run it to replace the secrets (a renewed certificate, a new API key).
#
# 1. The certificate, as a .p12. In Keychain Access, open "My Certificates", right-click
#    "Developer ID Application: <your name> (<team id>)" and choose Export. Export only that
#    one item (it includes its private key), as .p12, with a password. Delete the .p12 once
#    this script is done with it.
#
# 2. Notarization, one of:
#    - An App Store Connect API key (recommended: no Apple ID, and it can be revoked alone).
#      App Store Connect > Users and Access > Integrations > App Store Connect API > Team Keys,
#      generate a key with the Developer role and download its AuthKey_<key id>.p8 (only
#      possible once). The issuer ID is shown above the keys table.
#    - An Apple ID with an app-specific password from account.apple.com > Sign-In and
#      Security > App-Specific Passwords, and the team ID.
#
# The updater signing key (TAURI_SIGNING_PRIVATE_KEY) is not handled here; see RELEASING.md.
if [ -z "${BASH_VERSION:-}" ]; then exec bash "$0" "$@"; fi
set -euo pipefail

REPO="${REPO:-emircan-sahin/gitviber}"
ENVIRONMENT=release

die() {
  echo "error: $*" >&2
  exit 1
}

[ -t 0 ] || die "run this in a terminal; it asks for passwords"
command -v gh > /dev/null || die "gh (GitHub CLI) is not installed"
command -v openssl > /dev/null || die "openssl is not installed"
gh auth status > /dev/null 2>&1 || die "gh is not signed in: gh auth login"
gh api "repos/$REPO/environments/$ENVIRONMENT" --silent 2> /dev/null ||
  die "$REPO has no \"$ENVIRONMENT\" environment (Settings > Environments), or you can't see it"

# Each prints the answer; prompts go to stderr, so `x=$(ask …)` shows them.
ask() {
  local value
  read -r -p "$1: " value
  printf '%s' "$value"
}

ask_secret() {
  local value
  read -r -s -p "$1: " value
  echo >&2
  printf '%s' "$value"
}

# The file path as typed, or dragged into the terminal (quoted, or with escaped spaces).
ask_file() {
  local path
  path=$(ask "$1")
  path="${path#\'}" path="${path%\'}" path="${path#\"}" path="${path%\"}" path="${path//\\ / }"
  path="${path/#\~/$HOME}"
  [ -f "$path" ] || die "no such file: $path"
  printf '%s' "$path"
}

# --- Certificate -----------------------------------------------------------------------------

p12=$(ask_file "Path of the Developer ID .p12")
P12_PASSWORD=$(ask_secret "Its export password")
export P12_PASSWORD

# OpenSSL 3 needs -legacy for the RC2 encryption older Keychain Access exports use; LibreSSL
# (macOS's /usr/bin/openssl) has no such flag and reads both.
pkcs12() {
  openssl pkcs12 -in "$p12" -passin env:P12_PASSWORD "$@" 2> /dev/null ||
    openssl pkcs12 -legacy -in "$p12" -passin env:P12_PASSWORD "$@" 2> /dev/null
}
certs=$(pkcs12 -nokeys -clcerts) || die "can't open $p12: wrong password, or not a .p12"
[ "$(printf '%s\n' "$certs" | grep -c 'BEGIN CERTIFICATE')" = 1 ] ||
  die "the .p12 must hold exactly one certificate; export only the Developer ID Application item"
# Counted through the pipe, so the key itself never lands in a variable.
[ "$(pkcs12 -nocerts -nodes | grep -c 'PRIVATE KEY-----')" -ge 1 ] ||
  die "the .p12 has no private key; export the certificate from My Certificates, not Certificates"
printf '%s\n' "$certs" | openssl x509 -noout -checkend 0 > /dev/null || die "the certificate has expired"

identity=$(printf '%s\n' "$certs" | openssl x509 -noout -subject -nameopt multiline |
  sed -n 's/^ *commonName *= *//p')
case "$identity" in
  "Developer ID Application: "*) ;;
  "")
    echo "Couldn't read the certificate's name."
    identity=$(ask 'Signing identity, e.g. "Developer ID Application: Your Name (TEAMID)"')
    ;;
  *) die "\"$identity\" is not a Developer ID Application certificate, which notarization needs" ;;
esac
cert_team=$(printf '%s' "$identity" | sed -n 's/.*(\([A-Z0-9]\{10\}\))$/\1/p')
echo "Certificate: $identity"

# --- Notarization ----------------------------------------------------------------------------

echo
echo "Notarize with:"
echo "  1) an App Store Connect API key (recommended)"
echo "  2) an Apple ID and an app-specific password"
method=$(ask "1 or 2")

case "$method" in
  1)
    p8=$(ask_file "Path of the AuthKey_<key id>.p8")
    grep -q 'BEGIN PRIVATE KEY' "$p8" || die "$p8 is not an App Store Connect API key (.p8)"
    key_id=$(basename "$p8" | sed -n 's/^AuthKey_\([A-Z0-9]\{10\}\)\.p8$/\1/p')
    if [ -z "$key_id" ]; then key_id=$(ask "Key ID (10 characters)"); fi
    [[ "$key_id" =~ ^[A-Z0-9]{10}$ ]] || die "a key ID is 10 letters and digits, not \"$key_id\""
    issuer=$(ask "Issuer ID")
    [[ "$issuer" =~ ^[0-9a-fA-F]{8}-([0-9a-fA-F]{4}-){3}[0-9a-fA-F]{12}$ ]] ||
      die "an issuer ID looks like 69a6de70-…, not \"$issuer\""
    summary="API key $key_id, issuer $issuer"
    ;;
  2)
    apple_id=$(ask "Apple ID (email)")
    [[ "$apple_id" =~ ^[^@[:space:]]+@[^@[:space:]]+\.[^@[:space:]]+$ ]] || die "\"$apple_id\" isn't an email address"
    apple_password=$(ask_secret "App-specific password (xxxx-xxxx-xxxx-xxxx)")
    [[ "$apple_password" =~ ^[a-z]{4}-[a-z]{4}-[a-z]{4}-[a-z]{4}$ ]] ||
      die "that isn't an app-specific password; make one at account.apple.com"
    team_id=$(ask "Team ID [${cert_team:-none}]")
    team_id="${team_id:-$cert_team}"
    [[ "$team_id" =~ ^[A-Z0-9]{10}$ ]] || die "a team ID is 10 letters and digits, not \"$team_id\""
    summary="Apple ID $apple_id, team $team_id"
    ;;
  *) die "pick 1 or 2" ;;
esac

# --- Set -------------------------------------------------------------------------------------

echo
echo "This sets, in $REPO's \"$ENVIRONMENT\" environment:"
echo "  APPLE_CERTIFICATE, APPLE_CERTIFICATE_PASSWORD, APPLE_SIGNING_IDENTITY ($identity)"
echo "  notarization: $summary"
go=$(ask "Go ahead? [y/N]")
[ "$go" = y ] || [ "$go" = Y ] || die "nothing was set"

set_secret() { # set_secret <name>, value on stdin
  gh secret set "$1" --repo "$REPO" --env "$ENVIRONMENT" > /dev/null
  echo "  set $1"
}

# Switching methods: the workflow would still see the other method's old secrets.
if [ "$method" = 1 ]; then stale="APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID"; else stale="APPLE_API_PRIVATE_KEY APPLE_API_KEY APPLE_API_ISSUER"; fi
existing=$(gh secret list --repo "$REPO" --env "$ENVIRONMENT" --json name --jq '.[].name')
for name in $stale; do
  if printf '%s\n' "$existing" | grep -qx "$name"; then
    gh secret delete "$name" --repo "$REPO" --env "$ENVIRONMENT"
    echo "  deleted $name"
  fi
done

base64 < "$p12" | tr -d '\n' | set_secret APPLE_CERTIFICATE
printf '%s' "$P12_PASSWORD" | set_secret APPLE_CERTIFICATE_PASSWORD
printf '%s' "$identity" | set_secret APPLE_SIGNING_IDENTITY
if [ "$method" = 1 ]; then
  set_secret APPLE_API_PRIVATE_KEY < "$p8"
  printf '%s' "$key_id" | set_secret APPLE_API_KEY
  printf '%s' "$issuer" | set_secret APPLE_API_ISSUER
else
  printf '%s' "$apple_id" | set_secret APPLE_ID
  printf '%s' "$apple_password" | set_secret APPLE_PASSWORD
  printf '%s' "$team_id" | set_secret APPLE_TEAM_ID
fi

echo
echo "Done. Delete $p12 now that it's stored. To check signing end to end, push a test tag"
echo "(see RELEASING.md): git tag v<version>-rc.1 && git push origin v<version>-rc.1"
