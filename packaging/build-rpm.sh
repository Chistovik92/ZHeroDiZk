#!/bin/sh
# Build zherodizk-server-<version>-1.<arch>.rpm from already built static binaries.
# Usage: build-rpm.sh <bin-dir> <version> <rpm-arch> <out-dir>
set -eu
bin=$(CDPATH= cd -- "$1" && pwd); version=$2; arch=$3; out=$4
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
top=$(mktemp -d)
trap 'rm -rf "$top"' EXIT
sed -e "s/@VERSION@/$version/" -e "s#@BINDIR@#$bin#" -e "s#@SRCDIR@#$here#" \
    "$here/rpm/zherodizk-server.spec" > "$top/server.spec"
mkdir -p "$out"
rpmbuild -bb --target "$arch" --define "_topdir $top" --define "_rpmdir $(CDPATH= cd -- "$out" && pwd)" \
    --define "_build_id_links none" --define "__strip /bin/true" "$top/server.spec"
