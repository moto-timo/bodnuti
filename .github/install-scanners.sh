#!/bin/sh
# Installs pinned trivy and gitleaks release binaries into /usr/local/bin,
# checking each against its release's checksum file. Pinned here, not taken
# from a third-party action, so a CI run only ever runs these versions.
set -eu

TRIVY=0.76.1
GITLEAKS=8.30.1

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
cd "$tmp"

fetch() { # url
	curl -fsSLO "$1"
}

fetch "https://github.com/aquasecurity/trivy/releases/download/v$TRIVY/trivy_${TRIVY}_Linux-64bit.tar.gz"
fetch "https://github.com/aquasecurity/trivy/releases/download/v$TRIVY/trivy_${TRIVY}_checksums.txt"
grep " trivy_${TRIVY}_Linux-64bit.tar.gz\$" "trivy_${TRIVY}_checksums.txt" | sha256sum -c -
tar -xzf "trivy_${TRIVY}_Linux-64bit.tar.gz" trivy

fetch "https://github.com/gitleaks/gitleaks/releases/download/v$GITLEAKS/gitleaks_${GITLEAKS}_linux_x64.tar.gz"
fetch "https://github.com/gitleaks/gitleaks/releases/download/v$GITLEAKS/gitleaks_${GITLEAKS}_checksums.txt"
grep " gitleaks_${GITLEAKS}_linux_x64.tar.gz\$" "gitleaks_${GITLEAKS}_checksums.txt" | sha256sum -c -
tar -xzf "gitleaks_${GITLEAKS}_linux_x64.tar.gz" gitleaks

sudo install -m 755 trivy gitleaks /usr/local/bin/
trivy --version
gitleaks version
