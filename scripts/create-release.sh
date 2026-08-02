#!/usr/bin/env bash

set -eu -o pipefail

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" >/dev/null && pwd )"
cd "$SCRIPT_DIR/.."

version=${1:-}
if [[ -z "$version" ]]; then
    echo "USAGE: $0 version" >&2
    exit 1
fi

if [[ "$(git symbolic-ref --short HEAD)" != "main" ]]; then
    echo "must be on main branch" >&2
    exit 1
fi

# Check if tag already exists
if git rev-parse "${version}" &>/dev/null; then
    echo "Tag ${version} already exists" >&2
    exit 1
fi

# Update version in Cargo.toml
sed -i -e "0,/^version = / s!^version = \".*\"!version = \"${version}\"!" Cargo.toml

# Update distribution packaging in contrib/
sed -i -e "s/^pkgver=.*/pkgver=${version}/" contrib/arch/PKGBUILD contrib/alpine/APKBUILD
sed -i -e "s/^Version: .*/Version:        ${version}/" contrib/fedora/cntr.spec
sed -i -e "/^%changelog/a * $(LC_ALL=C date '+%a %b %d %Y') Jörg Thalheim <joerg@thalheim.io> - ${version}-1\n- Update to ${version}\n" contrib/fedora/cntr.spec
cat > contrib/debian/changelog.new <<EOF
cntr (${version}-1) unstable; urgency=medium

  * Update to ${version}

 -- Jörg Thalheim <joerg@thalheim.io>  $(LC_ALL=C date -R)

EOF
cat contrib/debian/changelog >> contrib/debian/changelog.new
mv contrib/debian/changelog.new contrib/debian/changelog
git mv contrib/gentoo/app-containers/cntr/cntr-*.ebuild "contrib/gentoo/app-containers/cntr/cntr-${version}.ebuild"

# Update Cargo.lock
cargo build --release

# Create release branch and PR
git checkout -b "release-${version}"
git add Cargo.toml Cargo.lock contrib
git commit -m "bump version to ${version}"
git push --set-upstream origin "release-${version}"

gh pr create \
    --title "Release ${version}" \
    --body "Bump version to ${version}" \
    --base main

gh pr merge --auto --merge

# Wait for PR to be merged
echo "Waiting for PR to be merged..."
while [[ "$(gh pr view --json state --jq '.state')" != "MERGED" ]]; do
    sleep 5
done

# Go back to main and pull changes
git checkout main
git pull origin main

# Create draft release which will trigger the publish workflow
gh release create "${version}" --draft --title "${version}" --generate-notes
