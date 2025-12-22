#!/bin/bash
set -euo pipefail

dnf install -y rpm-build rpmdevtools dnf-plugins-core
dnf builddep -y ~/rpmbuild/SPECS/cntr.spec
rpmbuild -ba ~/rpmbuild/SPECS/cntr.spec
