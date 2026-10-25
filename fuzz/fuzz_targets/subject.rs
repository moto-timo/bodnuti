//! Subject tag and `k/N` counter parsing, which routing and branch
//! selection rely on.

#![no_main]

use bodnuti::series::{counters, subject_tags};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|subject: &str| {
    let tags = subject_tags(subject);
    let (_version, index, total) = counters(&tags);
    let _ = (index, total);
});
