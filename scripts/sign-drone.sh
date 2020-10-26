#!/usr/bin/env nix-shell
#!nix-shell -i bash -p bash -p drone-cli -p sops
set -xeuo pipefail

DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" >/dev/null 2>&1 && pwd )"

eval "$(sops -d --output-type dotenv ${DIR}/drone-secrets.yml)"
cd "$DIR"/..
export DRONE_SERVER=https://drone.thalheim.io DRONE_TOKEN
drone sign Mic92/cntr --save
git add .drone.yml
