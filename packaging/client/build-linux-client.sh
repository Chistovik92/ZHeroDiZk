#!/bin/sh
# Build tar.gz, deb and rpm packages of the Linux client from a built bundle.
# Usage: build-linux-client.sh <bundle-dir> <version> <out-dir>
# The bundle is vendor/client/flutter/build/linux/x64/release/bundle (plus the launcher files).
set -eu
bundle=$(CDPATH= cd -- "$1" && pwd); version=$2; out=$3
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
root=$(CDPATH= cd -- "$here/../.." && pwd)
mkdir -p "$out"
out=$(CDPATH= cd -- "$out" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

png="$work/zherodizk.png"
python3 -c "import sys; sys.path.insert(0, sys.argv[1]); import make_icons; open(sys.argv[2], 'wb').write(make_icons.png_bytes(256))" "$root/tools" "$png"

# --- tar.gz: the bundle as it is, with licences and a note ---
tarroot="$work/tar/zherodizk-client-$version-linux-x86_64"
mkdir -p "$tarroot"
cp -a "$bundle/." "$tarroot/"
cp "$root/LICENSE" "$root/NOTICE" "$tarroot/"
cp "$here/README-linux.txt" "$tarroot/README.txt"
tar -C "$work/tar" -czf "$out/zherodizk-client-$version-linux-x86_64.tar.gz" "zherodizk-client-$version-linux-x86_64"

# --- deb ---
deb="$work/deb"
install -d "$deb/DEBIAN" "$deb/opt/zherodizk" "$deb/usr/bin" "$deb/usr/share/applications" \
    "$deb/usr/share/icons/hicolor/256x256/apps" "$deb/usr/share/doc/zherodizk-client"
cp -a "$bundle/." "$deb/opt/zherodizk/"
install -m 0755 "$here/zherodizk-wrapper" "$deb/usr/bin/zherodizk"
install -m 0644 "$here/zherodizk.desktop" "$deb/usr/share/applications/zherodizk.desktop"
install -m 0644 "$png" "$deb/usr/share/icons/hicolor/256x256/apps/zherodizk.png"
cp "$root/LICENSE" "$root/NOTICE" "$deb/usr/share/doc/zherodizk-client/"
cp "$here/README-linux.txt" "$deb/usr/share/doc/zherodizk-client/README.txt"
sed -e "s/@VERSION@/$version/" "$here/control.in" > "$deb/DEBIAN/control"
dpkg-deb --root-owner-group --build "$deb" "$out/zherodizk-client_${version}_amd64.deb"

# --- rpm (no automatic dependency scan: the bundle carries its own libraries) ---
top="$work/rpm"
mkdir -p "$top"
sed -e "s/@VERSION@/$version/" -e "s#@BUNDLE@#$bundle#g" -e "s#@SRCDIR@#$here#g" -e "s#@ROOT@#$root#g" -e "s#@PNG@#$png#g" \
    "$here/zherodizk-client.spec" > "$top/client.spec"
rpmbuild -bb --target x86_64 --define "_topdir $top" --define "_rpmdir $out" \
    --define "_build_id_links none" --define "__strip /bin/true" \
    --define "__os_install_post %{nil}" "$top/client.spec"
find "$out" -mindepth 2 -type f -name '*.rpm' -exec mv {} "$out"/ \;
find "$out" -mindepth 1 -type d -empty -delete
