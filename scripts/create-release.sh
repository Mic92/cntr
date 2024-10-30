#!/usr/bin/env bash

set -eu -o pipefail

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" >/dev/null && pwd )"
cd "$SCRIPT_DIR/.."

version=${1:-}
if [[ -z "$version" ]]; then
    echo "USAGE: $0 version" 2>/dev/null
    exit 1
fi

if [[ "$(git symbolic-ref --short HEAD)" != "master" ]]; then
    echo "must be on master branch" 2>/dev/null
    exit 1
fi

sed -i -e "0,/version =/ s!^version = \".*\"!version = \"${version}\"!" Cargo.toml
git add Cargo.toml
cargo build --release
nix flake check -L
git add Cargo.lock
git commit -m "bump version to ${version}"
git tag "${version}"

echo "now run 'git push --tags origin master'"
