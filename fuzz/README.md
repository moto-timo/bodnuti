# Fuzz targets

bodnuti reads mail and lore feeds that anyone can write. These targets run
its parsers on arbitrary input with libFuzzer, through
[cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz):

| Target | What it runs |
|---|---|
| `mbox` | `Series::parse` on mbox data, then every method bodnuti calls on a parsed patch (quoted-printable decoding, touched files, From header) |
| `lore_feed` | `lore::parse` on an Atom feed, then grouping its entries into series; a reply must never name a series |
| `subject` | subject tags and `k/N` counters, which routing and branch selection rely on |

cargo-fuzz needs a nightly toolchain. The main crate stays on stable and
does not depend on this one.

```
rustup toolchain install nightly
cargo install cargo-fuzz
make fuzz                      # each target for FUZZ_SECONDS (default 60)
```

or one target by hand, from this directory:

```
cargo +nightly fuzz run -O mbox corpus/mbox seeds/mbox -- -max_total_time=600
```

`seeds/` holds the starting inputs and is committed. `corpus/`, which
libFuzzer grows, and `artifacts/`, where it writes any input that crashes,
are not. Turn a crash into a regression test in the module it belongs to,
then fix it.
