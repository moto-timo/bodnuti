//! request_pull::parse on arbitrary message text: what reaches git fetch
//! must have passed its validation.

#![no_main]

use bodnuti::request_pull;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| {
    if let Ok(pr) = request_pull::parse(text) {
        assert!(pr.url.starts_with("https://"));
        assert!(!pr.branch.starts_with('-') && !pr.branch.contains(".."));
        assert!(pr.base.len() == 40 && pr.head.len() == 40);
    }
});
