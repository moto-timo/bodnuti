//! The parts of bodnuti that read untrusted input on their own: mail
//! (`series`), pull requests (`request_pull`), the lore feed (`lore`) and
//! grouping messages into series (`track`). They are a library so that fuzz targets (`fuzz/`) can link
//! them; the `bodnuti` binary uses them from here.

pub mod lore;
pub mod request_pull;
pub mod series;
pub mod track;
