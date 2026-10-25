# bodnuti with every checker it drives, for hosts without them (and for
# macOS, where bitbake does not run). Build with:
#   docker build -t bodnuti -f Containerfile .

FROM docker.io/library/rust:1.90-slim-trixie AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY tests ./tests
RUN cargo build --release --locked

# shellcheck ships as a static binary in its own image.
FROM docker.io/koalaman/shellcheck:v0.10.0 AS shellcheck

FROM docker.io/library/debian:trixie-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends git python3 python3-venv ca-certificates \
 && rm -rf /var/lib/apt/lists/*

# Python tools in one venv: oelint-adv, b4, what patchtest's mbox and patch
# suites import (pyparsing, unidiff, GitPython, Jinja2), and the Sphinx
# packages yocto-docs' Pipfile lists, plus sphinx-lint.
RUN python3 -m venv /opt/venv \
 && /opt/venv/bin/pip install --no-cache-dir \
      oelint-adv==9.11.2 b4==0.16.0 \
      pyparsing==3.3.3 unidiff==1.0.1 GitPython==3.2.1 Jinja2==3.1.6 \
      Sphinx==9.2.0 sphinx-rtd-theme==3.1.0 sphinx-copybutton==0.5.2 \
      sphinxcontrib-svg2pdfconverter==2.1.0 PyYAML==6.0.3 sphinx-lint==1.0.2

COPY --from=shellcheck /bin/shellcheck /usr/local/bin/shellcheck
COPY --from=build /src/target/release/bodnuti /usr/local/bin/bodnuti

ENV PATH=/opt/venv/bin:$PATH \
    BODNUTI_SHELLCHECK=shellcheck \
    BODNUTI_PYTHON=/opt/venv/bin/python \
    BODNUTI_OELINT=/opt/venv/bin/oelint-adv \
    BODNUTI_B4=/opt/venv/bin/b4 \
    XDG_CACHE_HOME=/tmp/.cache

# Patches are untrusted input; nothing here needs root. XDG_CACHE_HOME
# points b4's cache at /tmp, so the image also runs with --read-only and a
# tmpfs on /tmp.
RUN useradd --create-home bodnuti \
 && git config --system --add safe.directory '*'
USER bodnuti
WORKDIR /work
ENTRYPOINT ["bodnuti"]
