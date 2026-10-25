//! Series::parse and everything bodnuti reads from a parsed patch, on
//! arbitrary mbox data: what `b4 mbox` returns for a thread anyone can post.

#![no_main]

use bodnuti::series::Series;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(series) = Series::parse(data) else {
        return;
    };
    let _ = series.branch_candidates();
    for patch in series.cover.iter().chain(&series.patches) {
        let _ = patch.is_cover_letter();
        let _ = patch.decoded();
        let _ = patch.files();
        let _ = patch.changes();
        let _ = patch.added_files();
        let _ = patch.deleted_files();
        let _ = patch.summary();
        let _ = patch.author();
        let _ = patch.author_email();
    }
});
