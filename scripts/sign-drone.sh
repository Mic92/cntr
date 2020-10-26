#!/usr/bin/env nix-shell
#!nix-shell -i bash -p bash -p drone-cli -p sops
set -euo pipefail

eval "$(sops -d --output-type dotenv ./drone-secrets.yml)"
DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" >/dev/null 2>&1 && pwd )"
cd "$DIR"/..
export DRONE_SERVER=https://drone.thalheim.io DRONE_TOKEN
drone sign Mic92/dotfiles --save
git add .drone.yml
