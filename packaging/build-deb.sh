#!/bin/sh
# Build zherodizk-server_<version>_<arch>.deb from already built static binaries.
# Usage: build-deb.sh <bin-dir> <version> <deb-arch> <out-dir>
set -eu
bin=$1; version=$2; arch=$3; out=$4
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
root=$(mktemp -d)
trap 'rm -rf "$root"' EXIT
install -d "$root/DEBIAN" "$root/usr/bin" "$root/lib/systemd/system" "$root/etc/zherodizk"
for b in zhd-rendezvous zhd-relay zhd-utils; do install -m 0755 "$bin/$b" "$root/usr/bin/$b"; done
install -m 0644 "$here"/systemd/*.service "$root/lib/systemd/system/"
install -m 0640 "$here/common/server.env" "$root/etc/zherodizk/server.env"
sed -e "s/@VERSION@/$version/" -e "s/@ARCH@/$arch/" "$here/deb/control.in" > "$root/DEBIAN/control"
echo /etc/zherodizk/server.env > "$root/DEBIAN/conffiles"
install -m 0755 "$here/deb/postinst" "$here/deb/prerm" "$root/DEBIAN/"
mkdir -p "$out"
dpkg-deb --root-owner-group --build "$root" "$out/zherodizk-server_${version}_${arch}.deb"
