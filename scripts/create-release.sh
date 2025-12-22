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

# Ensure working directory is clean
uncommitted_changes=$(git diff --compact-summary)
if [[ -n "$uncommitted_changes" ]]; then
    echo -e "There are uncommitted changes, exiting:\n${uncommitted_changes}" >&2
    exit 1
fi

unpushed_changes=$(git log --format=oneline origin/main..main)
if [[ -n "$unpushed_changes" ]]; then
    echo -e "There are unpushed changes, exiting:\n${unpushed_changes}" >&2
    exit 1
fi

# Check if tag already exists
if git rev-parse "${version}" &>/dev/null; then
    echo "Tag ${version} already exists" >&2
    exit 1
fi

# Update version in Cargo.toml
sed -i -e "0,/version =/ s!^version = \".*\"!version = \"${version}\"!" Cargo.toml

# Update Cargo.lock
cargo build --release

# Run checks
nix flake check -L

# Create release branch and PR
git checkout -b "release-${version}"
git add Cargo.toml Cargo.lock
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
