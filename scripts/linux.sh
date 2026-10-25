#!/bin/bash
# The Linux release on Ubuntu 22.04 (the oldest glibc we support), into target/dist/linux/:
#
#   scripts/linux.sh build [--profile P]   the app, release profile by default
#   scripts/linux.sh deb                   GitViber_<version>_amd64.deb (dpkg-deb, dpkg-shlibdeps)
#   scripts/linux.sh rpm                   GitViber-<version>-1.x86_64.rpm (rpmbuild)
#   scripts/linux.sh appimage              GitViber_<version>_amd64.AppImage (mksquashfs and the
#                                          AppImage project's runtime, pinned below)
#
# The package is git-viber, as the Tauri bundles named it, so a new one upgrades the old.
set -euo pipefail
cd "$(dirname "$0")/.."

OUT=target/dist/linux
BIN="$OUT/gitviber"
PACKAGE=git-viber
SUMMARY="Vibe code from one app: worktrees, live diffs, a file explorer and a terminal."
DESCRIPTION="GitViber shows full-file diffs that update live as agents write, every worktree one click away, and a terminal right under the diff. It runs the git CLI with your own config, hooks, credentials and signing."
HOMEPAGE=https://github.com/emircan-sahin/gitviber
# A dated release of github.com/AppImage/type2-runtime; its checksum, as GitHub lists it.
RUNTIME_URL=https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-x86_64
RUNTIME_SHA256=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d

version() { sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml; }
die() {
  echo "linux.sh: $*" >&2
  exit 1
}

build() {
  local profile=release
  while [ $# -gt 0 ]; do
    case $1 in
      --profile) profile=$2; shift ;;
      *) die "build: unknown option $1" ;;
    esac
    shift
  done
  cargo build -p gitviber --locked --profile "$profile"
  mkdir -p "$OUT"
  local dir=$profile
  [ "$profile" = dev ] && dir=debug
  cp "target/$dir/gitviber" "$BIN"
}

# usr/ as every package holds it: the binary, the desktop file, the icons.
usr_tree() {
  local root=$1
  [ -f "$BIN" ] || die "no binary; run scripts/linux.sh build first"
  install -Dm755 "$BIN" "$root/usr/bin/gitviber"
  install -Dm644 packaging/linux/GitViber.desktop "$root/usr/share/applications/GitViber.desktop"
  install -Dm644 packaging/icons/32x32.png "$root/usr/share/icons/hicolor/32x32/apps/gitviber.png"
  install -Dm644 packaging/icons/128x128.png "$root/usr/share/icons/hicolor/128x128/apps/gitviber.png"
  install -Dm644 packaging/icons/128x128@2x.png "$root/usr/share/icons/hicolor/256x256/apps/gitviber.png"
}

deb() {
  local root="$OUT/deb" deb
  deb="$OUT/GitViber_$(version)_amd64.deb"
  rm -rf "$root" && mkdir -p "$root/DEBIAN"
  usr_tree "$root"
  # The libraries the binary links, as the packages that hold them; plus the Vulkan loader, which
  # the renderer opens at run time, so no link shows it.
  local shlibs
  shlibs=$(mkdir -p "$OUT/shlibs/debian" && cd "$OUT/shlibs" &&
    printf 'Source: %s\n\nPackage: %s\n' "$PACKAGE" "$PACKAGE" > debian/control &&
    dpkg-shlibdeps -O "$OLDPWD/$root/usr/bin/gitviber" | sed -n 's/^shlibs:Depends=//p')
  rm -rf "$OUT/shlibs"
  [ -n "$shlibs" ] || die "dpkg-shlibdeps found no dependencies"
  cat > "$root/DEBIAN/control" <<EOF
Package: $PACKAGE
Version: $(version)
Architecture: amd64
Installed-Size: $(du -sk "$root/usr" | cut -f1)
Maintainer: Emircan Sahin
Priority: optional
Homepage: $HOMEPAGE
Depends: $shlibs, libvulkan1
Description: $SUMMARY
 $DESCRIPTION
EOF
  (cd "$root" && find usr -type f -exec md5sum {} + | sort -k 2) > "$root/DEBIAN/md5sums"
  dpkg-deb --root-owner-group -Zgzip --build "$root" "$deb"
  rm -rf "$root"
  dpkg-deb --info "$deb"
}

rpm() {
  local top
  top=$(mktemp -d)
  usr_tree "$top/root"
  # Each library the binary links, as the capability rpm-based systems name it; the Vulkan loader
  # the renderer opens at run time too. Listed here rather than left to rpmbuild, whose scanners
  # vary with the host.
  local requires
  requires=$( (objdump -p "$BIN" | awk '$1 == "NEEDED" { print $2 }'; echo libvulkan.so.1) |
    sort -u | sed 's/$/()(64bit)/' | paste -sd, - | sed 's/,/, /g')
  [ -n "$requires" ] || die "objdump found no libraries"
  cat > "$top/gitviber.spec" <<EOF
Name: $PACKAGE
Version: $(version)
Release: 1
Summary: $SUMMARY
License: GPL-3.0-or-later
URL: $HOMEPAGE
AutoReqProv: no
Requires: $requires
%define debug_package %{nil}
%define __os_install_post %{nil}
%define _build_id_links none
%define _binary_payload w9.gzdio

%description
$DESCRIPTION

%install
mkdir -p "%{buildroot}"
cp -a "$top/root/." "%{buildroot}/"

%files
/usr/bin/gitviber
/usr/share/applications/GitViber.desktop
/usr/share/icons/hicolor/32x32/apps/gitviber.png
/usr/share/icons/hicolor/128x128/apps/gitviber.png
/usr/share/icons/hicolor/256x256/apps/gitviber.png
EOF
  rpmbuild -bb --target x86_64 --define "_topdir $top/build" --define "_rpmdir $top/out" "$top/gitviber.spec"
  mkdir -p "$OUT"
  mv "$top/out/x86_64/$PACKAGE-$(version)-1.x86_64.rpm" "$OUT/GitViber-$(version)-1.x86_64.rpm"
  rm -rf "$top"
  echo "$OUT/GitViber-$(version)-1.x86_64.rpm"
}

appimage() {
  local dir="$OUT/AppDir" image runtime="$OUT/runtime-x86_64"
  image="$OUT/GitViber_$(version)_amd64.AppImage"
  rm -rf "$dir" && mkdir -p "$dir"
  usr_tree "$dir"
  # The image's root, as the runtime and desktop integrations look for them.
  ln -s usr/bin/gitviber "$dir/AppRun"
  cp packaging/linux/GitViber.desktop "$dir/gitviber.desktop"
  cp packaging/icons/128x128@2x.png "$dir/gitviber.png"
  ln -s gitviber.png "$dir/.DirIcon"
  # Every folder and program executable by anyone: root mounts it, another user runs it.
  chmod -R a+rX,go-w "$dir"
  [ -f "$runtime" ] || curl -fsSL -o "$runtime" "$RUNTIME_URL"
  echo "$RUNTIME_SHA256  $runtime" | sha256sum -c -
  mksquashfs "$dir" "$OUT/AppDir.squashfs" -root-owned -noappend -comp zstd -quiet
  cat "$runtime" "$OUT/AppDir.squashfs" > "$image"
  chmod 755 "$image"
  rm -rf "$dir" "$OUT/AppDir.squashfs"
  echo "$image"
}

command=${1:-}
[ $# -gt 0 ] && shift
case $command in
  build) build "$@" ;;
  deb) deb ;;
  rpm) rpm ;;
  appimage) appimage ;;
  *) die "usage: scripts/linux.sh build|deb|rpm|appimage" ;;
esac
