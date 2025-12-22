#!/bin/bash
set -euo pipefail

if [ ! -d /var/db/repos/gentoo/metadata ]; then
  emerge-webrsync
fi

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

# Use emerge to handle BDEPEND (rust-bin, scdoc) automatically
emerge --oneshot =app-containers/cntr-${version}
