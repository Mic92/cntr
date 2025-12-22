#!/bin/bash
set -euo pipefail

pacman -Syu --noconfirm base-devel

useradd -m builder || true
echo "builder ALL=(ALL) NOPASSWD: ALL" >> /etc/sudoers

chown -R builder:builder .
su builder -c "makepkg -sf --noconfirm --syncdeps"
