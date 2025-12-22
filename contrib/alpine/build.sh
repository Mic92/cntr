#!/bin/sh
set -eu

apk update
apk add --no-cache alpine-sdk sudo

adduser -D builder || true
addgroup builder abuild || true
echo "builder ALL=(ALL) NOPASSWD: ALL" >> /etc/sudoers

mkdir -p /var/cache/distfiles
chmod a+w /var/cache/distfiles
chown -R builder:builder .

su builder -c "abuild-keygen -an"
cp /home/builder/.abuild/*.pub /etc/apk/keys/
su builder -c "abuild checksum"
su builder -c "abuild -r"

mkdir -p dist
find /home/builder/packages -name '*.apk' -exec cp {} dist/ \;
