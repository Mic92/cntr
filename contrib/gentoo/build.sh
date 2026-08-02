#!/bin/bash
set -euo pipefail

# Refresh the (cached) portage tree; a stale tree does not match the
# eclasses and toolchain versions expected by the current stage3 image.
emerge-webrsync

version=$(grep "^version" /src/Cargo.toml | head -1 | cut -d'"' -f2)

mkdir -p /var/db/repos/local/{metadata,profiles,app-containers/cntr}
echo "local" > /var/db/repos/local/profiles/repo_name
echo "masters = gentoo" > /var/db/repos/local/metadata/layout.conf

mkdir -p /etc/portage/repos.conf
cat > /etc/portage/repos.conf/local.conf <<EOF
[local]
location = /var/db/repos/local
EOF

cp /src/contrib/gentoo/app-containers/cntr/*.ebuild /var/db/repos/local/app-containers/cntr/

cd /var/db/repos/local/app-containers/cntr
ebuild "cntr-${version}.ebuild" manifest

# The stage3 container resolves the package without scheduling its BDEPEND,
# so install the build tools explicitly before building the package.
emerge --oneshot dev-lang/rust-bin app-text/scdoc
emerge --oneshot =app-containers/cntr-${version}
