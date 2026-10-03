#!/bin/sh
# Build zherodizk-control-<version>-1.<arch>.rpm from an already built static binary.
# Usage: build-control-rpm.sh <binary> <version> <rpm-arch> <out-dir>
set -eu
binary=$(CDPATH= cd -- "$(dirname -- "$1")" && pwd)/$(basename -- "$1"); version=$2; arch=$3; out=$4
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
top=$(mktemp -d)
trap 'rm -rf "$top"' EXIT
sed -e "s/@VERSION@/$version/" -e "s#@BINARY@#$binary#" -e "s#@SRCDIR@#$here#" \
    "$here/control/zherodizk-control.spec" > "$top/control.spec"
mkdir -p "$out"
rpmbuild -bb --target "$arch" --define "_topdir $top" --define "_rpmdir $(CDPATH= cd -- "$out" && pwd)" \
    --define "_build_id_links none" --define "__strip /bin/true" "$top/control.spec"
