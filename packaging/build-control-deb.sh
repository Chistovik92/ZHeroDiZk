#!/bin/sh
# Build zherodizk-control_<version>_<arch>.deb from an already built static binary.
# Usage: build-control-deb.sh <binary> <version> <deb-arch> <out-dir>
set -eu
binary=$1; version=$2; arch=$3; out=$4
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
root=$(mktemp -d)
trap 'rm -rf "$root"' EXIT
install -d "$root/DEBIAN" "$root/usr/bin" "$root/lib/systemd/system" "$root/etc/zherodizk" "$root/usr/share/doc/zherodizk-control"
install -m 0755 "$binary" "$root/usr/bin/zherodizk-control"
install -m 0644 "$here/control/zherodizk-control.service" "$root/lib/systemd/system/"
install -m 0640 "$here/control/control.env" "$root/etc/zherodizk/control.env"
install -m 0644 "$here/control/Caddyfile" "$here/control/nginx.conf" "$root/usr/share/doc/zherodizk-control/"
sed -e "s/@VERSION@/$version/" -e "s/@ARCH@/$arch/" "$here/control/deb-control.in" > "$root/DEBIAN/control"
echo /etc/zherodizk/control.env > "$root/DEBIAN/conffiles"
install -m 0755 "$here/control/postinst" "$here/control/prerm" "$root/DEBIAN/"
mkdir -p "$out"
dpkg-deb --root-owner-group --build "$root" "$out/zherodizk-control_${version}_${arch}.deb"
