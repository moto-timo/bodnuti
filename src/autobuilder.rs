//! The Yocto Project AutoBuilder's builds of staging branches, traced to the
//! series they carried (designs/PHASE2.md §5.7).
//!
//! The OE-Core maintainers push stacks of patches under review to staging
//! branches and start full builds (`a-full`), each of which triggers some
//! sixty builds across machines, distros and tests. A build's properties
//! name the commit it built of each layer, and poky-ci-archive keeps a tag
//! of the OE-Core and BitBake trees of every build, so the stack outlives
//! the branch's rebases. Its commits on top of their branch are matched to
//! checked series by patch-id, and the build, with its failures, is
//! recorded on each series it carried.
//!
//! A build tests some seventy patches at once, and some failures are no
//! patch's: a failed fetch is infrastructure, never a series'. A task
//! failure in a recipe a series changes is the series' at once; any other
//! failure only once two builds agree, both newly failing where the build
//! of their branch before had not, and both newly carrying the series.
//!
//! The AutoBuilder's API is public JSON. Everything from it is untrusted:
//! commits are checked to be object names, tags and branch names to be
//! plain, and log text is only ever shown, escaped.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::git;
use crate::watch::{Config, ListState};

const API: &str = "https://autobuilder.yoctoproject.org/valkyrie/api/v2";
const WEB: &str = "https://autobuilder.yoctoproject.org/valkyrie/#";
const ARCHIVE: &str = "https://git.yoctoproject.org/poky-ci-archive";

/// The repositories traced, with the name the AutoBuilder's properties use
/// for each, and whether poky-ci-archive keeps a tag of its tree per build.
/// A tree without one is read from the clone, which keeps the commits it
/// fetched from its staging branches, or from the build's own branch.
const LAYERS: [(&str, &str, bool); 3] = [
    ("openembedded-core", "oecore", true),
    ("bitbake", "bitbake", true),
    ("meta-openembedded", "meta-openembedded", false),
];

/// How long between looks at the AutoBuilder.
const INTERVAL_SECS: u64 = 30 * 60;

/// Builds looked at per builder and look, newest first.
const LIMIT: u32 = 10;

/// Characters of an error line kept.
const ERROR_WIDTH: usize = 300;

/// Where to look, from `[autobuilder]` in the configuration.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutobuilderConfig {
    /// The top-level builders whose builds are read, such as `a-full`.
    pub builders: Vec<String>,
}

/// A build as recorded on a series it carried.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AbBuild {
    pub builder: String,
    pub number: u64,
    pub url: String,
    /// The OE-Core or BitBake branch it built (`mathieu/master-next`).
    pub branch: String,
    /// `success`, `warnings`, `failure`, `cancelled` and so on.
    pub result: String,
    pub completed: String,
    /// The series' patches the build carried, by number.
    pub patches: Vec<u32>,
    /// How many series the build carried, this one included.
    pub series: usize,
    pub failures: Vec<AbFailure>,
}

/// One failed step of one of the build's builds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AbFailure {
    /// The builder of the failed build (`qemuarm64`).
    pub builder: String,
    pub step: String,
    pub url: String,
    /// The recipe file of the failed task, relative to its layer.
    pub recipe: Option<String>,
    pub task: Option<String>,
    /// The first error line.
    pub error: String,
    /// A failure of the infrastructure, such as a fetch: no series'.
    pub infra: bool,
    /// Tied to this series: by its recipe, or by two builds.
    #[serde(default)]
    pub tied: Option<Tie>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tie {
    /// The series changes the failed task's recipe.
    Recipe,
    /// Two builds failed this way newly carrying the series.
    Builds,
}

#[derive(Deserialize)]
struct ApiBuilds {
    builds: Vec<ApiBuild>,
}

#[derive(Deserialize)]
struct ApiBuild {
    buildid: u64,
    number: u64,
    #[serde(default)]
    builderid: u64,
    #[serde(default)]
    buildrequestid: u64,
    results: Option<u32>,
    complete_at: Option<u64>,
    #[serde(default)]
    properties: BTreeMap<String, (serde_json::Value, String)>,
}

impl ApiBuild {
    fn property(&self, name: &str) -> Option<&str> {
        self.properties.get(name)?.0.as_str()
    }
}

/// What `state/autobuilder.json` keeps between polls.
#[derive(Debug, Default, Serialize, Deserialize)]
struct AbState {
    last_poll: u64,
    /// The newest build handled of each builder.
    last_number: BTreeMap<String, u64>,
    /// The last build handled of each `<layer> <branch>`.
    last: BTreeMap<String, Seen>,
    /// How many builds failed newly carrying each `<list>/<root> <failure>`.
    new_failures: BTreeMap<String, u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Seen {
    stack: BTreeSet<String>,
    failures: BTreeSet<String>,
}

/// Looks at the AutoBuilder for finished builds, at most every
/// `INTERVAL_SECS`, and records each on the series it carried.
pub fn poll(config: &Config, now: u64) -> Result<()> {
    let Some(ab) = &config.autobuilder else {
        return Ok(());
    };
    let path = config.state.join("autobuilder.json");
    let mut state: AbState = match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).context("parsing autobuilder.json")?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => AbState::default(),
        Err(e) => return Err(e).context("reading autobuilder.json"),
    };
    if now < state.last_poll + INTERVAL_SECS {
        return Ok(());
    }
    state.last_poll = now;
    let layers: Vec<(&str, &str, bool)> = LAYERS
        .into_iter()
        .filter(|(repo, _, _)| config.repos.contains_key(*repo) && config.cache.join(repo).is_dir())
        .collect();
    if layers.is_empty() {
        return Ok(());
    }
    let names = builder_names()?;
    let mut lists: BTreeMap<String, ListState> = BTreeMap::new();
    for builder in &ab.builders {
        let Some(id) = names.iter().find(|(_, n)| *n == builder).map(|(id, _)| *id) else {
            eprintln!("bodnuti: the AutoBuilder has no builder {builder}");
            continue;
        };
        let props: String = layers
            .iter()
            .flat_map(|(_, l, _)| {
                [
                    format!("&property=branch_{l}"),
                    format!("&property=commit_{l}"),
                    format!("&property=repo_{l}"),
                ]
            })
            .collect();
        let url = format!(
            "{API}/builders/{id}/builds?order=-number&limit={LIMIT}&complete=true\
             &property=poky-ci-archive_tag{props}"
        );
        let mut builds: Vec<ApiBuild> = serde_json::from_str::<ApiBuilds>(&get(&url)?)
            .context("parsing the AutoBuilder's builds")?
            .builds;
        builds.sort_by_key(|b| b.number);
        for build in builds {
            if state
                .last_number
                .get(builder)
                .is_some_and(|&n| n >= build.number)
            {
                continue;
            }
            state.last_number.insert(builder.clone(), build.number);
            let failures = if matches!(build.results, Some(0) | None) {
                Vec::new()
            } else {
                failures(&build, &names).unwrap_or_else(|err| {
                    eprintln!("bodnuti: {builder} {}: {err:#}", build.number);
                    Vec::new()
                })
            };
            for (repo, layer, archived) in &layers {
                let tree = Tree {
                    repo,
                    layer,
                    archived: *archived,
                };
                record_build(
                    config, &mut state, &mut lists, builder, id, &build, &tree, &failures,
                )?;
            }
        }
    }
    for (list, list_state) in &lists {
        list_state.save(&config.state, list)?;
    }
    std::fs::write(&path, serde_json::to_vec_pretty(&state)?).context("writing autobuilder.json")
}

/// One repository's tree in a build.
struct Tree<'a> {
    repo: &'a str,
    layer: &'a str,
    archived: bool,
}

/// Keeps every staging branch tip each clone has fetched under
/// `refs/bodnuti/seen/`, so that the commit a later build names stays in the
/// clone after the branch is rebased and git gc runs: the AutoBuilder keeps
/// no archive of meta-openembedded's trees (designs/PHASE2.md §5.7).
pub fn pin_staging_tips(config: &Config) {
    for (name, repo) in &config.repos {
        if repo.staging.is_empty() {
            continue;
        }
        let clone = config.cache.join(name);
        let Ok(tips) = git::run(
            &clone,
            &[
                "for-each-ref",
                "--format=%(objectname)",
                "refs/remotes/staging/",
                "refs/remotes/origin/*-next",
            ],
        ) else {
            continue;
        };
        for tip in tips.lines().filter(|t| is_object_name(t)) {
            let pin = format!("refs/bodnuti/seen/{tip}");
            if git::run(&clone, &["rev-parse", "--verify", "--quiet", &pin]).is_err() {
                let _ = git::run(&clone, &["update-ref", &pin, tip]);
            }
        }
    }
}

/// Records `build`'s stack of `tree` on the series it carried.
#[allow(clippy::too_many_arguments)]
fn record_build(
    config: &Config,
    state: &mut AbState,
    lists: &mut BTreeMap<String, ListState>,
    builder: &str,
    builder_id: u64,
    build: &ApiBuild,
    tree: &Tree,
    failures: &[AbFailure],
) -> Result<()> {
    let (repo, layer) = (tree.repo, tree.layer);
    let (Some(branch), Some(commit)) = (
        build.property(&format!("branch_{layer}")),
        build.property(&format!("commit_{layer}")),
    ) else {
        return Ok(());
    };
    // A build may name its commit only as HEAD: its tree cannot be traced.
    if !is_object_name(commit) || !plain(branch) {
        return Ok(());
    }
    let tag = build
        .property("poky-ci-archive_tag")
        .filter(|t| tree.archived && plain(t));
    let from = build
        .property(&format!("repo_{layer}"))
        .and_then(fetch_url)
        .map(|url| (url, branch));
    let clone = config.cache.join(repo);
    let Some(stack) = stack(&clone, layer, tag, from, commit, branch)? else {
        return Ok(());
    };
    // Kept, so that a staging branch's rebase and git gc cannot take the
    // tree a recorded build tested.
    let _ = git::run(
        &clone,
        &[
            "update-ref",
            &format!(
                "refs/bodnuti/autobuilder/{builder}-{}-{layer}",
                build.number
            ),
            commit,
        ],
    );
    if stack.is_empty() {
        return Ok(());
    }
    let key = format!("{layer} {branch}");
    let previous = state.last.get(&key).cloned().unwrap_or_default();
    let keys: BTreeSet<String> = failures
        .iter()
        .filter(|f| !f.infra)
        .map(failure_key)
        .collect();
    let carried = crate::ci::carried(&config.state, repo, &stack)?;
    for (series, patches) in &carried {
        let newly_carried = patches.iter().any(|(id, _)| !previous.stack.contains(id));
        let changed = changed_dirs(&series_mbox(config, series));
        let mut shown = failures.to_vec();
        for f in &mut shown {
            if f.infra {
                continue;
            }
            if f.recipe
                .as_deref()
                .is_some_and(|r| changed.contains(parent(r)))
            {
                f.tied = Some(Tie::Recipe);
                continue;
            }
            let key = failure_key(f);
            if newly_carried && !previous.failures.contains(&key) {
                let count = state
                    .new_failures
                    .entry(format!("{series} {key}"))
                    .or_default();
                *count += 1;
                if *count >= 2 {
                    f.tied = Some(Tie::Builds);
                }
            }
        }
        let record = AbBuild {
            builder: builder.to_string(),
            number: build.number,
            url: format!("{WEB}/builders/{builder_id}/builds/{}", build.number),
            branch: branch.to_string(),
            result: result_name(build.results).into(),
            completed: crate::lore::utc(build.complete_at.unwrap_or(0)),
            patches: patches.iter().map(|(_, k)| *k).collect(),
            series: carried.len(),
            failures: shown,
        };
        add_build(config, lists, series, record)?;
    }
    state.last.insert(
        key,
        Seen {
            stack: stack.into_iter().collect(),
            failures: keys,
        },
    );
    Ok(())
}

/// What makes two failures the same in two builds: where, and in what.
fn failure_key(f: &AbFailure) -> String {
    match (&f.recipe, &f.task) {
        (Some(r), Some(t)) => format!("{}|{r}:{t}", f.builder),
        _ => format!("{}|{}", f.builder, f.step),
    }
}

/// The directory of a recipe file, which holds its other files too.
fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(d, _)| d)
}

/// The recipe directories the series' patches change: each changed file's
/// directory, and for a file in a recipe's `files/` or `<recipe>/`
/// subdirectory (`piglit/piglit/0001-fix.patch`), the recipe's.
fn changed_dirs(series: &Option<crate::series::Series>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let Some(series) = series else { return out };
    for patch in &series.patches {
        for file in patch.files() {
            let dir = parent(&file);
            out.insert(dir.to_string());
            let above = parent(dir);
            let name = dir.rsplit('/').next().unwrap_or("");
            if name == "files" || above.rsplit('/').next() == Some(name) {
                out.insert(above.to_string());
            }
        }
    }
    out.remove("");
    out
}

/// The series' patches, from its mbox in the state directory.
fn series_mbox(config: &Config, series: &str) -> Option<crate::series::Series> {
    let (list, root) = series.split_once('/')?;
    if !crate::lore::safe_id(root) {
        return None;
    }
    let mbox = config
        .state
        .join("series")
        .join(list)
        .join(root)
        .join(format!("{root}.mbx"));
    crate::series::Series::load_rooted(&[&mbox], root).ok()
}

/// The https URL to fetch a build's repository from: the AutoBuilder names
/// the push URL (`ssh://git@push.openembedded.org/meta-openembedded-contrib`)
/// of the two hosts it builds from. Anything else is not fetched.
fn fetch_url(repo: &str) -> Option<String> {
    let path = repo
        .strip_prefix("ssh://git@push.openembedded.org/")
        .map(|p| ("git.openembedded.org", p))
        .or_else(|| {
            repo.strip_prefix("ssh://git@push.yoctoproject.org/")
                .map(|p| ("git.yoctoproject.org", p))
        })
        .or_else(|| {
            repo.strip_prefix("https://git.openembedded.org/")
                .map(|p| ("git.openembedded.org", p))
        })
        .or_else(|| {
            repo.strip_prefix("https://git.yoctoproject.org/")
                .map(|p| ("git.yoctoproject.org", p))
        });
    let (host, path) = path?;
    let path = path.trim_end_matches(".git");
    plain(path).then(|| format!("https://{host}/{path}"))
}

/// The patch-ids of the commits the build's tree of `layer` has on top of
/// the branch its staging branch was forked from. The commit comes from the
/// clone when it has it, as it does for a staging branch it fetched, else
/// from poky-ci-archive's tag of the build, else from the build's own
/// branch (`from`). None when none of them has it, as after a rebase of a
/// branch the clone never fetched.
fn stack(
    clone: &Path,
    layer: &str,
    tag: Option<&str>,
    from: Option<(String, &str)>,
    commit: &str,
    branch: &str,
) -> Result<Option<Vec<String>>> {
    let have = |c: &str| git::run(clone, &["cat-file", "-e", &format!("{c}^{{commit}}")]).is_ok();
    if !have(commit)
        && let Some(tag) = tag
    {
        let refspec = format!("refs/tags/{layer}/{tag}");
        let _ = git::run(
            clone,
            &["fetch", "--quiet", "--no-tags", "--", ARCHIVE, &refspec],
        );
    }
    if !have(commit)
        && let Some((url, branch)) = &from
    {
        let refspec = format!("refs/heads/{branch}");
        let _ = git::run(
            clone,
            &["fetch", "--quiet", "--no-tags", "--", url, &refspec],
        );
    }
    if !have(commit) {
        return Ok(None);
    }
    let target = target_branch(clone, branch);
    let log = git::command(clone)
        .args(["log", "-p", "--no-merges", "--no-color"])
        .arg(format!("origin/{target}..{commit}"))
        .output()
        .context("running git log")?;
    Ok(Some(
        crate::workspace::patch_id_lines(&log.stdout)?
            .into_iter()
            .map(|(id, _)| id)
            .collect(),
    ))
}

/// The branch a staging branch stages: the release a name like
/// `stable/scarthgap-nut` or `pbarker/certs-wrynose` names, when the clone
/// has that branch, else `master`.
fn target_branch(clone: &Path, branch: &str) -> String {
    let words: Vec<&str> = branch
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty() && *w != "master")
        .collect();
    for word in words {
        if git::run(
            clone,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("origin/{word}"),
            ],
        )
        .is_ok()
        {
            return word.to_string();
        }
    }
    "master".into()
}

/// The failed steps of the build's builds, with their first error.
fn failures(build: &ApiBuild, names: &BTreeMap<u64, String>) -> Result<Vec<AbFailure>> {
    #[derive(Deserialize)]
    struct Buildsets {
        buildsets: Vec<Bs>,
    }
    #[derive(Deserialize)]
    struct Bs {
        bsid: u64,
    }
    #[derive(Deserialize)]
    struct Requests {
        buildrequests: Vec<Req>,
    }
    #[derive(Deserialize)]
    struct Req {
        buildrequestid: u64,
    }
    #[derive(Deserialize)]
    struct Steps {
        steps: Vec<Step>,
    }
    #[derive(Deserialize)]
    struct Step {
        stepid: u64,
        name: String,
    }
    #[derive(Deserialize)]
    struct Logs {
        logs: Vec<Log>,
    }
    #[derive(Deserialize)]
    struct Log {
        logid: u64,
        name: String,
    }

    let sets: Buildsets = serde_json::from_str(&get(&format!(
        "{API}/buildsets?parent_buildid={}",
        build.buildid
    ))?)?;
    let mut requests = BTreeSet::new();
    for set in sets.buildsets {
        let page: Requests = serde_json::from_str(&get(&format!(
            "{API}/buildrequests?buildsetid__eq={}&limit=200",
            set.bsid
        ))?)?;
        requests.extend(page.buildrequests.into_iter().map(|r| r.buildrequestid));
    }
    // A build that triggers none, such as meta-oe's, failed in its own steps.
    let failed: Vec<ApiBuild> = match (requests.first(), requests.last()) {
        (Some(lo), Some(hi)) => {
            let children: ApiBuilds = serde_json::from_str(&get(&format!(
                "{API}/builds?buildrequestid__ge={lo}&buildrequestid__le={hi}&limit=500"
            ))?)?;
            children
                .builds
                .into_iter()
                .filter(|c| requests.contains(&c.buildrequestid) && c.results == Some(2))
                .collect()
        }
        _ => {
            let own: ApiBuilds =
                serde_json::from_str(&get(&format!("{API}/builds/{}", build.buildid))?)?;
            own.builds
                .into_iter()
                .filter(|b| b.results == Some(2))
                .collect()
        }
    };
    let mut out = Vec::new();
    for child in failed {
        let builder = names
            .get(&child.builderid)
            .cloned()
            .unwrap_or_else(|| child.builderid.to_string());
        let url = format!("{WEB}/builders/{}/builds/{}", child.builderid, child.number);
        let steps: Steps = serde_json::from_str(&get(&format!(
            "{API}/builds/{}/steps?results__eq=2",
            child.buildid
        ))?)?;
        for step in steps.steps {
            let logs: Logs =
                serde_json::from_str(&get(&format!("{API}/steps/{}/logs", step.stepid))?)?;
            let text = match logs.logs.iter().find(|l| l.name == "errors") {
                Some(log) => get(&format!("{API}/logs/{}/raw", log.logid))?,
                None => String::new(),
            };
            out.push(failure_of(&builder, &step.name, &url, &text));
        }
    }
    Ok(out)
}

/// A failed step from its `errors` log: the recipe and task of the first
/// failed task, the first error line, and whether it was a fetch.
fn failure_of(builder: &str, step: &str, url: &str, errors: &str) -> AbFailure {
    let lines: Vec<&str> = errors
        .lines()
        .map(|l| l.strip_prefix("stdio: ").unwrap_or(l).trim())
        .collect();
    let error: String = lines
        .iter()
        .find(|l| l.starts_with("ERROR:"))
        .map(|l| l.chars().take(ERROR_WIDTH).collect())
        .unwrap_or_default();
    // ERROR: Task (<path>/<repository>/<recipe>.bb:do_<task>) failed ...,
    // the repository checked out as layers/openembedded-core in a full
    // build, or as meta-openembedded beside it in meta-oe's.
    let (recipe, task) = lines
        .iter()
        .find_map(|l| {
            let rest = l.strip_prefix("ERROR: Task (")?;
            let (target, _) = rest.split_once(')')?;
            let (path, task) = target.rsplit_once(':')?;
            let relative = LAYERS
                .iter()
                .filter_map(|(repo, _, _)| {
                    let at = path.rfind(&format!("/{repo}/"))?;
                    Some((at, &path[at + repo.len() + 2..]))
                })
                .max_by_key(|(at, _)| *at)
                .map(|(_, rest)| rest)?;
            Some((Some(relative.to_string()), Some(task.to_string())))
        })
        .unwrap_or((None, None));
    let infra = task.as_deref() == Some("do_fetch")
        || error.contains("Fetcher failure")
        || error.contains("FetchError");
    AbFailure {
        builder: builder.into(),
        step: step.into(),
        url: url.into(),
        recipe,
        task,
        error,
        infra,
        tied: None,
    }
}

fn builder_names() -> Result<BTreeMap<u64, String>> {
    #[derive(Deserialize)]
    struct Builders {
        builders: Vec<B>,
    }
    #[derive(Deserialize)]
    struct B {
        builderid: u64,
        name: String,
    }
    let page: Builders = serde_json::from_str(&get(&format!("{API}/builders"))?)?;
    Ok(page
        .builders
        .into_iter()
        .map(|b| (b.builderid, b.name))
        .collect())
}

fn result_name(results: Option<u32>) -> &'static str {
    match results {
        Some(0) => "success",
        Some(1) => "warnings",
        Some(2) => "failure",
        Some(3) => "skipped",
        Some(4) => "exception",
        Some(5) => "retry",
        Some(6) => "cancelled",
        _ => "unknown",
    }
}

fn is_object_name(sha: &str) -> bool {
    sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit())
}

/// A tag or branch name made of plain characters, never an option.
fn plain(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && !name.contains("..")
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-/".contains(&b))
}

fn get(url: &str) -> Result<String> {
    let mut response = ureq::get(url)
        .header("User-Agent", crate::lore::USER_AGENT)
        .config()
        .timeout_global(Some(std::time::Duration::from_secs(60)))
        .build()
        .call()
        .with_context(|| format!("fetching {url}"))?;
    response
        .body_mut()
        .read_to_string()
        .context("reading the AutoBuilder's answer")
}

/// Adds `build` to the record of `series` (`<list>/<root>`), once.
fn add_build(
    config: &Config,
    lists: &mut BTreeMap<String, ListState>,
    series: &str,
    build: AbBuild,
) -> Result<()> {
    let Some((list, root)) = series.split_once('/') else {
        return Ok(());
    };
    if !lists.contains_key(list) {
        lists.insert(list.to_string(), ListState::load(&config.state, list)?);
    }
    let Some(record) = lists.get_mut(list).and_then(|s| s.series.get_mut(root)) else {
        return Ok(());
    };
    if !record
        .autobuilder
        .iter()
        .any(|b| b.builder == build.builder && b.number == build.number)
    {
        record.autobuilder.push(build);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_failed_step_from_its_errors_log() {
        let fetch = "stdio: ERROR: sbom-cve-check-update-cvelist-native-2026-09-12-r0 do_fetch: \
            Fetcher failure: Fetch command ['git', 'fetch'] failed with exit code 128\n\
            stdio: ERROR: Task (/srv/pokybuild/yocto-worker/qemuarm64-armhost/build/layers/\
            openembedded-core/meta/recipes-devtools/sbom-cve-check/\
            sbom-cve-check-update-cvelist-native_2026-09-12.bb:do_fetch) failed with exit code '1'\n";
        let f = failure_of("qemuarm64-armhost", "Build tar", "u", fetch);
        assert_eq!(
            f.recipe.as_deref(),
            Some(
                "meta/recipes-devtools/sbom-cve-check/sbom-cve-check-update-cvelist-native_2026-09-12.bb"
            )
        );
        assert_eq!(f.task.as_deref(), Some("do_fetch"));
        assert!(f.infra);
        assert!(f.error.starts_with("ERROR: sbom-cve-check"));

        let compile = "stdio: ERROR: piglit-1.0-r0 do_compile: oe_runmake failed\n\
            stdio: ERROR: Task (/srv/w/build/layers/openembedded-core/meta/recipes-graphics/\
            piglit/piglit_git.bb:do_compile) failed with exit code '1'\n";
        let f = failure_of("qemux86-64", "Build images", "u", compile);
        assert!(!f.infra);
        assert_eq!(
            failure_key(&f),
            "qemux86-64|meta/recipes-graphics/piglit/piglit_git.bb:do_compile"
        );
        assert_eq!(
            parent(f.recipe.as_deref().unwrap()),
            "meta/recipes-graphics/piglit"
        );

        // meta-oe's builder checks meta-openembedded out beside the build.
        let meta_oe = "stdio: ERROR: Task (/srv/pokybuild/yocto-worker/meta-oe/build/\
            meta-openembedded/meta-oe/recipes-support/pcp/pcp-native_6.3.7.bb:do_compile) \
            failed with exit code '1'\n";
        let f = failure_of("meta-oe", "Build targets", "u", meta_oe);
        assert_eq!(
            f.recipe.as_deref(),
            Some("meta-oe/recipes-support/pcp/pcp-native_6.3.7.bb")
        );
        assert_eq!(f.task.as_deref(), Some("do_compile"));

        // A test failure names no task.
        let f = failure_of("oe-selftest-debian", "Run cmds: oe-selftest", "u", "");
        assert_eq!(failure_key(&f), "oe-selftest-debian|Run cmds: oe-selftest");
    }

    #[test]
    fn a_recipe_is_its_directory_and_its_patch_directories() {
        let mail = "From 1 Mon Sep 17 00:00:00 2001\nFrom: A <a@x>\nSubject: [PATCH] piglit: fix\n\n\
            fix\n---\n\
            diff --git a/meta/recipes-graphics/piglit/piglit_git.bb b/meta/recipes-graphics/piglit/piglit_git.bb\n\
            --- a/meta/recipes-graphics/piglit/piglit_git.bb\n+++ b/meta/recipes-graphics/piglit/piglit_git.bb\n\
            @@ -1 +1 @@\n-a\n+b\n\
            diff --git a/meta/recipes-graphics/piglit/piglit/0001-fix.patch b/meta/recipes-graphics/piglit/piglit/0001-fix.patch\n\
            new file mode 100644\n--- /dev/null\n+++ b/meta/recipes-graphics/piglit/piglit/0001-fix.patch\n\
            @@ -0,0 +1 @@\n+x\n";
        let series = crate::series::Series::parse(mail.as_bytes()).ok();
        let dirs = changed_dirs(&series);
        assert!(dirs.contains("meta/recipes-graphics/piglit"), "{dirs:?}");
        // Not every recipe beside it.
        assert!(!dirs.contains("meta/recipes-graphics"), "{dirs:?}");
    }

    #[test]
    fn fetches_a_build_s_repository_over_https() {
        assert_eq!(
            fetch_url("ssh://git@push.openembedded.org/meta-openembedded-contrib").as_deref(),
            Some("https://git.openembedded.org/meta-openembedded-contrib")
        );
        assert_eq!(
            fetch_url("https://git.openembedded.org/openembedded-core-contrib").as_deref(),
            Some("https://git.openembedded.org/openembedded-core-contrib")
        );
        assert_eq!(
            fetch_url("ssh://git@push.yoctoproject.org/meta-virtualization").as_deref(),
            Some("https://git.yoctoproject.org/meta-virtualization")
        );
        assert_eq!(fetch_url("https://github.com/aws/meta-aws.git"), None);
        assert_eq!(
            fetch_url("ssh://git@push.openembedded.org/--upload-pack=x"),
            None
        );
    }

    #[test]
    fn checks_names_from_the_autobuilder() {
        assert!(plain("autobuilder.yoctoproject.org/valkyrie/a-full-5004"));
        assert!(plain("mathieu/master-next"));
        assert!(!plain("--upload-pack=x"));
        assert!(!plain("a/../b"));
        assert!(!plain("a b"));
        assert!(is_object_name("ae5cafacec8e38e77746e32458e09461f12737b6"));
        assert!(!is_object_name("HEAD"));
    }

    #[test]
    fn finds_the_branch_a_staging_branch_stages() {
        let up = tempfile::tempdir().unwrap();
        let p = up.path();
        git::run(p, &["init", "-q", "-b", "master"]).unwrap();
        git::run(
            p,
            &[
                "-c",
                "user.name=T",
                "-c",
                "user.email=t@x",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "x",
            ],
        )
        .unwrap();
        git::run(p, &["branch", "scarthgap"]).unwrap();
        let clone = tempfile::tempdir().unwrap();
        git::run(clone.path(), &["clone", "-q", p.to_str().unwrap(), "."]).unwrap();
        let c = clone.path();
        assert_eq!(target_branch(c, "stable/scarthgap-nut"), "scarthgap");
        assert_eq!(target_branch(c, "pbarker/certs-scarthgap"), "scarthgap");
        assert_eq!(target_branch(c, "mathieu/master-next"), "master");
        assert_eq!(target_branch(c, "stable/wrynose-nut"), "master");
    }
}
