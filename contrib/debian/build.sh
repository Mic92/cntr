#!/bin/bash
set -euo pipefail

apt-get update
apt-get install -y devscripts equivs

mk-build-deps --install --remove --tool "apt-get -y" debian/control
rustc --version
cargo --version
dpkg-buildpackage -us -uc -b

mkdir -p dist
cp ../*.deb dist/
