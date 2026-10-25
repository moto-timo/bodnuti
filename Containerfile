# bodnuti with every checker it drives, for hosts without them (and for
# macOS, where bitbake does not run). Build with:
#   docker build -t bodnuti -f Containerfile .

FROM docker.io/library/rust:1.90-slim-trixie AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY assets ./assets
COPY src ./src
COPY tests ./tests
RUN cargo build --release --locked

# shellcheck ships as a static binary in its own image.
FROM docker.io/koalaman/shellcheck:v0.10.0 AS shellcheck

# So does trivy, which scans Django applications' requirements pins. Its
# database is not in the image: the watcher downloads it daily and mounts it
# read-only at /trivy.
FROM docker.io/aquasec/trivy:0.76.1 AS trivy

FROM docker.io/library/debian:trixie-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends git python3 python3-venv ca-certificates \
      cppcheck \
 && rm -rf /var/lib/apt/lists/*

# Python tools in one venv. The pins live in requirements.txt, where
# vulnerability scanners and Dependabot can read them.
COPY requirements.txt /tmp/requirements.txt
RUN python3 -m venv /opt/venv \
 && /opt/venv/bin/pip install --no-cache-dir -r /tmp/requirements.txt \
 && rm /tmp/requirements.txt

COPY --from=shellcheck /bin/shellcheck /usr/local/bin/shellcheck
COPY --from=trivy /usr/local/bin/trivy /usr/local/bin/trivy
COPY --from=build /src/target/release/bodnuti /usr/local/bin/bodnuti

ENV PATH=/opt/venv/bin:$PATH \
    BODNUTI_SHELLCHECK=shellcheck \
    BODNUTI_PYTHON=/opt/venv/bin/python \
    BODNUTI_OELINT=/opt/venv/bin/oelint-adv \
    BODNUTI_B4=/opt/venv/bin/b4 \
    BODNUTI_RUFF=/opt/venv/bin/ruff \
    BODNUTI_DJLINT=/opt/venv/bin/djlint \
    BODNUTI_TRIVY=trivy \
    BODNUTI_TRIVY_DB=/trivy \
    XDG_CACHE_HOME=/tmp/.cache

# Patches are untrusted input; nothing here needs root. XDG_CACHE_HOME
# points b4's cache at /tmp, so the image also runs with --read-only and a
# tmpfs on /tmp.
RUN useradd --create-home bodnuti \
 && git config --system --add safe.directory '*'
USER bodnuti
WORKDIR /work
ENTRYPOINT ["bodnuti"]
