# Vulnerability and secret scans, and fuzzing. Tools: cargo-audit (cargo
# install cargo-audit), trivy and gitleaks (packaged by Homebrew and most
# distributions), and for fuzzing nightly Rust and cargo-fuzz.

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

FUZZ_SECONDS ?= 60

.PHONY: fuzz

# Each fuzz target in turn (fuzz/README.md). Needs nightly and cargo-fuzz.
fuzz:
	cd fuzz && for t in mbox lore_feed subject; do \
		mkdir -p corpus/$$t && \
		cargo +nightly fuzz run -O $$t corpus/$$t seeds/$$t -- \
			-max_total_time=$(FUZZ_SECONDS) -max_len=65536 || exit 1; \
	done
