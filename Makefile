# Vulnerability and secret scans. Tools: cargo-audit (cargo install
# cargo-audit), trivy and gitleaks (packaged by Homebrew and most
# distributions).

IMAGE ?= localhost/bodnuti

.PHONY: audit audit-image

# Dependencies as pinned (Cargo.lock, requirements.txt) and the whole git
# history for committed secrets. Each tool exits non-zero on a finding.
audit:
	cargo audit
	trivy fs --quiet --scanners vuln --exit-code 1 --skip-dirs target .
	gitleaks git --no-banner .

# The built image: Debian packages, the Python venv and the binary. Only
# findings with a fix are reported; the rest wait for Debian, and a rebuild
# picks up the fixes when they land.
audit-image:
	trivy image --quiet --scanners vuln --ignore-unfixed \
		--severity HIGH,CRITICAL --exit-code 1 $(IMAGE)
