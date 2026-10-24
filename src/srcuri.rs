//! Files a series stops using but leaves in the layer.
//!
//! When a recipe drops `file://0001-fix.patch` from `SRC_URI`, usually in a
//! version upgrade whose patch went upstream, the patch file itself must be
//! deleted too. patchtest's `test_src_uri_left_files` checks this through
//! tinfoil, but only for recipes the default MACHINE parses; a BSP recipe
//! such as barebox is skipped there. This check reads the diff and the tree
//! instead, needs no bitbake, and runs no code from the patch.

use std::path::Path;

use crate::finding::{Finding, Severity, Tool};
use crate::series::Series;

/// A `file://` name the series removed from a metadata file.
#[derive(Debug, PartialEq)]
struct Dropped {
    patch: u32,
    /// Directory of the metadata file the entry was removed from.
    dir: String,
    name: String,
}

fn is_metadata(path: &str) -> bool {
    [".bb", ".bbappend", ".inc"]
        .iter()
        .any(|ext| path.ends_with(ext))
}

/// `file://` names on the removed lines of each metadata file in a diff.
fn dropped(series: &Series) -> Vec<Dropped> {
    let mut out = Vec::new();
    for patch in &series.patches {
        let text = String::from_utf8_lossy(&patch.raw);
        let mut file: Option<String> = None;
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("diff --git a/") {
                file = rest.split_once(" b/").map(|(a, _)| a.to_string());
                continue;
            }
            let Some(path) = file.as_deref().filter(|p| is_metadata(p)) else {
                continue;
            };
            if line.starts_with("---") || !line.starts_with('-') {
                continue;
            }
            for name in file_uris(line) {
                let dir = Path::new(path)
                    .parent()
                    .map_or(String::new(), |d| d.display().to_string());
                out.push(Dropped {
                    patch: patch.index,
                    dir,
                    name,
                });
            }
        }
    }
    out
}

/// The paths in `file://` entries on a line, without parameters.
fn file_uris(line: &str) -> Vec<String> {
    line.split("file://")
        .skip(1)
        .filter_map(|rest| {
            let end = rest
                .find(|c: char| c.is_whitespace() || matches!(c, ';' | '"' | '\\' | '\''))
                .unwrap_or(rest.len());
            let name = &rest[..end];
            // A ${VAR} name cannot be resolved without bitbake.
            (!name.is_empty() && !name.contains('$')).then(|| name.to_string())
        })
        .collect()
}

/// Reports names the series removed that no metadata file in the recipe
/// directory still uses, but whose file is still in that directory.
pub fn leftover_files(tree: &Path, series: &Series) -> Vec<Finding> {
    let mut findings = Vec::new();
    for d in dropped(series) {
        let recipe_dir = tree.join(&d.dir);
        let mut metadata = Vec::new();
        let mut matches = Vec::new();
        walk(&recipe_dir, &mut |path| {
            let rel = path
                .strip_prefix(tree)
                .unwrap_or(path)
                .display()
                .to_string();
            if is_metadata(&rel) {
                metadata.push(path.to_path_buf());
            } else if rel.ends_with(&format!("/{}", d.name)) {
                matches.push(rel);
            }
        });
        let still_used = metadata.iter().any(|m| {
            std::fs::read_to_string(m).is_ok_and(|text| file_uris(&text).contains(&d.name))
        });
        if still_used {
            continue;
        }
        for file in matches {
            findings.push(Finding {
                tool: Tool::Bodnuti,
                check: "bodnuti.srcuri.leftover-file".into(),
                severity: Severity::Warning,
                patch: Some(d.patch),
                message: format!(
                    "file://{} was removed from SRC_URI and no recipe in {} uses it any more, but the file is still in the layer",
                    d.name, d.dir
                ),
                file: Some(file),
                line: None,
            });
        }
    }
    findings
}

fn walk(dir: &Path, visit: &mut dyn FnMut(&Path)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, visit);
        } else {
            visit(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn series(diff: &str) -> Series {
        let raw = format!(
            "From x Mon Sep 17 00:00:00 2001\nSubject: [PATCH] barebox: upgrade\n\n---\n{diff}"
        );
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), raw).unwrap();
        Series::load(&[file.path()]).unwrap()
    }

    const DIFF: &str = "diff --git a/meta/recipes-bsp/barebox/barebox.bb b/meta/recipes-bsp/barebox/barebox.bb\n\
                        --- a/meta/recipes-bsp/barebox/barebox.bb\n\
                        +++ b/meta/recipes-bsp/barebox/barebox.bb\n\
                        @@ -1,3 +1,1 @@\n\
                        -SRC_URI += \"file://0001-lib-lz4-use-lz4.patch \\\n\
                        -           \"\n";

    #[test]
    fn reads_dropped_names_from_the_diff() {
        assert_eq!(
            dropped(&series(DIFF)),
            vec![Dropped {
                patch: 1,
                dir: "meta/recipes-bsp/barebox".into(),
                name: "0001-lib-lz4-use-lz4.patch".into()
            }]
        );
        assert_eq!(
            file_uris("SRC_URI = \"file://a.patch;striplevel=2 file://${B}/x\""),
            vec!["a.patch"]
        );
    }

    fn tree(with_patch: bool, still_used: bool) -> tempfile::TempDir {
        let tree = tempfile::tempdir().unwrap();
        let dir = tree.path().join("meta/recipes-bsp/barebox");
        std::fs::create_dir_all(dir.join("barebox")).unwrap();
        let src = if still_used {
            "SRC_URI += \"file://0001-lib-lz4-use-lz4.patch\"\n"
        } else {
            "PV = \"2024.12.0\"\n"
        };
        std::fs::write(dir.join("barebox-common.inc"), src).unwrap();
        if with_patch {
            std::fs::write(dir.join("barebox/0001-lib-lz4-use-lz4.patch"), "x").unwrap();
        }
        tree
    }

    #[test]
    fn reports_a_patch_file_left_behind() {
        let t = tree(true, false);
        let findings = leftover_files(t.path(), &series(DIFF));
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].file.as_deref(),
            Some("meta/recipes-bsp/barebox/barebox/0001-lib-lz4-use-lz4.patch")
        );
        assert_eq!(findings[0].patch, Some(1));
    }

    #[test]
    fn a_deleted_or_still_used_file_is_fine() {
        assert!(leftover_files(tree(false, false).path(), &series(DIFF)).is_empty());
        assert!(leftover_files(tree(true, true).path(), &series(DIFF)).is_empty());
    }
}
