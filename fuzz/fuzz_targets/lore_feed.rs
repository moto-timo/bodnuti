//! lore::parse on arbitrary Atom feeds, then grouping the entries into
//! series as a poll does.

#![no_main]

use bodnuti::lore;
use bodnuti::track::Tracker;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(xml) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(entries) = lore::parse(xml) else {
        return;
    };
    for e in &entries {
        let _ = lore::parse_time(&e.updated);
        let _ = lore::safe_id(&e.message_id);
    }
    let mut tracker = Tracker::default();
    tracker.ingest(&entries);
    for series in tracker.series.values() {
        // A series is named by a patch, never by a reply to one.
        assert!(
            !series
                .subject
                .trim_start()
                .to_ascii_lowercase()
                .starts_with("re:")
        );
        let _ = series.complete();
    }
});
