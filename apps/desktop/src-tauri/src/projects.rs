use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::{fs, sync::Mutex};
use ts_rs::TS;

use crate::{
    events::now_rfc3339,
    store::{get_home_app_dir, read_json, write_atomic},
    Fail,
};

/// A directory the user attached, and the root a session runs in. Distinct from
/// [`crate::store::SessionIndexItem::project_path`], which records where a
/// session *did* run — a project can be detached without rewriting history.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "events.ts")]
#[serde(rename_all = "camelCase")]
pub struct Project {
    /// Canonicalized at attach time, so this is the only spelling of the path
    /// that ever reaches the index or the sidebar's grouping key.
    pub path: String,
    /// Folder name as of attaching. Cached so a project whose directory was
    /// since renamed or removed still has a label.
    pub name: String,
    /// Which space the project belongs to, or `None` for one nobody filed.
    /// The tag is the whole record of a space — there is no spaces file — so a
    /// space exists exactly while some project names it, and the last project
    /// leaving takes it with them.
    #[serde(default)]
    pub space: Option<String>,
    /// Doubles as the sort key and the "which project was last open" answer:
    /// selecting a project *is* what makes it most recent, so a separate
    /// `last_selected` pointer would be a second place to keep the same fact.
    pub last_selected: String,
}

static PROJECTS_LOCK: Mutex<()> = Mutex::const_new(());

/// Resolves symlinks and drops any trailing slash, so `/x/proj` and `/x/proj/`
/// can't become two projects and split the sidebar's grouping.
async fn canonical(path: &str) -> Result<String> {
    let resolved = fs::canonicalize(path)
        .await
        .with_context(|| format!("no such directory: {path}"))?;

    Ok(resolved.to_string_lossy().into_owned())
}

/// Reads `projects.json`, most recently selected first — so the picker's order
/// and its default are both just `projects[0]`. A missing or empty file means
/// no projects yet, not an error — same convention as the session index.
#[tauri::command]
pub async fn list_projects() -> Result<Vec<Project>, Fail> {
    let mut projects: Vec<Project> = read_json(&projects_path().await?).await?;
    // Descending, so the newest selection sorts to the front. RFC 3339 stamps
    // compare correctly as strings at fixed width.
    projects.sort_by(|a, b| b.last_selected.cmp(&a.last_selected));

    Ok(projects)
}

async fn projects_path() -> Result<std::path::PathBuf> {
    Ok(get_home_app_dir().await?.join("projects.json"))
}

/// Caller must hold `PROJECTS_LOCK`: this rewrites the whole file, so a
/// concurrent writer would drop the other's entry.
async fn write_projects(projects: &[Project]) -> Result<()> {
    write_atomic(&projects_path().await?, serde_json::to_string(projects)?).await
}

/// Attaches a directory and selects it. Re-attaching a known project is a
/// no-op apart from the selection, so the picker's "Attach" can double as
/// "switch to one I already have" without growing duplicates.
#[tauri::command]
pub async fn add_project(path: &str) -> Result<Vec<Project>, Fail> {
    let path = canonical(path).await?;

    let _guard = PROJECTS_LOCK.lock().await;
    let mut projects = list_projects().await?;
    let now = now_rfc3339();

    match projects.iter_mut().find(|p| p.path == path) {
        Some(existing) => existing.last_selected = now,
        None => projects.push(Project {
            name: basename(&path),
            path,
            space: None,
            last_selected: now,
        }),
    }

    projects.sort_by(|a, b| b.last_selected.cmp(&a.last_selected));
    write_projects(&projects).await?;

    Ok(projects)
}

/// Detaches a project. Sessions that ran in it are untouched — they keep their
/// own recorded paths and stay in the sidebar.
#[tauri::command]
pub async fn remove_project(path: &str) -> Result<Vec<Project>, Fail> {
    let _guard = PROJECTS_LOCK.lock().await;
    let mut projects = list_projects().await?;

    projects.retain(|p| p.path != path);
    write_projects(&projects).await?;

    Ok(projects)
}

/// Stamps a project as the most recently selected, which also moves it to the
/// front of the next read. Unknown paths are ignored rather than inserted —
/// attaching is [`add_project`]'s job.
#[tauri::command]
pub async fn set_last_selected_project(path: &str) -> Result<(), Fail> {
    let _guard = PROJECTS_LOCK.lock().await;
    let mut projects = list_projects().await?;

    let Some(project) = projects.iter_mut().find(|p| p.path == path) else {
        return Ok(());
    };

    project.last_selected = now_rfc3339();
    projects.sort_by(|a, b| b.last_selected.cmp(&a.last_selected));

    Ok(write_projects(&projects).await?)
}

/// Files a project under a space, or clears it with `None`. A blank name is
/// the same as clearing: an empty string would draw a nameless entry in the
/// switcher that nothing could ever be moved out of.
#[tauri::command]
pub async fn set_project_space(path: &str, space: Option<String>) -> Result<Vec<Project>, Fail> {
    let _guard = PROJECTS_LOCK.lock().await;
    let mut projects = list_projects().await?;

    // By index, not `iter_mut().find()`: the borrow checker will not let the
    // not-found arm hand the list back while a mutable borrow of it is alive.
    let Some(i) = projects.iter().position(|p| p.path == path) else {
        return Ok(projects);
    };

    projects[i].space = normalize_space(space);
    write_projects(&projects).await?;

    Ok(projects)
}

/// A blank name is the same as no space: an empty string would draw a nameless
/// entry in the switcher that nothing could ever be moved out of.
fn normalize_space(space: Option<String>) -> Option<String> {
    space.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// The edit [`retag_space`] makes, split from the file so it can be tested
/// without a `~/.dray` to write into. Answers whether anything moved.
fn retag(projects: &mut [Project], from: &str, to: Option<String>) -> bool {
    let to = normalize_space(to);
    let mut moved = false;

    for project in projects.iter_mut() {
        if project.space.as_deref() != Some(from) {
            continue;
        }
        project.space = to.clone();
        moved = true;
    }

    moved
}

/// Moves every project filed under one space to another, or out of any space
/// with `None` — a rename and a removal being the same operation.
///
/// One call rather than one per project, and that is the whole point: the
/// caller's own record of which spaces exist is updated beside this, so a run
/// of writes half of which failed would leave tags and that record describing
/// different worlds. Here it is one read, one edit and one write under the
/// lock, so it either all lands or none of it does.
#[tauri::command]
pub async fn retag_space(from: &str, to: Option<String>) -> Result<Vec<Project>, Fail> {
    let _guard = PROJECTS_LOCK.lock().await;
    let mut projects = list_projects().await?;

    // A space nobody had filled yet carries no tag, so changing nothing is the
    // ordinary path for renaming one — and a rewrite that moves no value is one
    // every other reader of this file has to survive for no reason.
    if retag(&mut projects, from, to) {
        write_projects(&projects).await?;
    }

    Ok(projects)
}

/// Trailing path segment. Mirrors the frontend's `basename` so a project's
/// cached label matches what the UI would derive from the path.
fn basename(path: &str) -> String {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(path)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn most_recently_selected_sorts_first() {
        let mut projects = vec![
            Project {
                path: "/a".into(),
                name: "a".into(),
                space: None,
                last_selected: "2026-08-01T00:00:00Z".into(),
            },
            Project {
                path: "/b".into(),
                name: "b".into(),
                space: None,
                last_selected: "2026-08-08T00:00:00Z".into(),
            },
        ];

        projects.sort_by(|a, b| b.last_selected.cmp(&a.last_selected));

        // The picker takes its default from the front, so this ordering is the
        // whole of "reopen the project I was last in".
        assert_eq!(projects[0].path, "/b");
    }

    #[test]
    fn a_project_written_before_spaces_existed_still_reads() {
        // The file is rewritten whole, so one entry failing to parse is the
        // whole index of projects gone.
        let project: Project = serde_json::from_str(
            r#"{"path":"/a","name":"a","lastSelected":"2026-08-01T00:00:00Z"}"#,
        )
        .unwrap();

        assert_eq!(project.space, None);
    }

    fn filed(path: &str, space: Option<&str>) -> Project {
        Project {
            path: path.into(),
            name: path.into(),
            space: space.map(Into::into),
            last_selected: "2026-08-01T00:00:00Z".into(),
        }
    }

    #[test]
    fn retag_moves_one_space_and_leaves_the_rest() {
        let mut projects = vec![
            filed("/a", Some("Work")),
            filed("/b", Some("Personal")),
            filed("/c", None),
            filed("/d", Some("Work")),
        ];

        assert!(retag(&mut projects, "Work", Some("Client".into())));
        let spaces: Vec<_> = projects.iter().map(|p| p.space.as_deref()).collect();
        assert_eq!(spaces, [Some("Client"), Some("Personal"), None, Some("Client")]);
    }

    #[test]
    fn retag_to_nothing_is_how_a_space_is_removed() {
        let mut projects = vec![filed("/a", Some("Work")), filed("/b", Some("Personal"))];

        assert!(retag(&mut projects, "Work", None));
        assert_eq!(projects[0].space, None);
        assert_eq!(projects[1].space.as_deref(), Some("Personal"));
    }

    #[test]
    fn retagging_a_space_no_project_carries_writes_nothing() {
        // A space made and not yet filled is renamed in the caller's own list
        // alone, so the file must not be rewritten to change nothing.
        let mut projects = vec![filed("/a", Some("Work"))];

        assert!(!retag(&mut projects, "Empty", Some("Renamed".into())));
        assert_eq!(projects[0].space.as_deref(), Some("Work"));
    }

    #[test]
    fn a_blank_name_files_a_project_under_nothing() {
        // Otherwise the switcher draws a nameless entry nothing can leave.
        assert_eq!(normalize_space(Some("  ".into())), None);
        assert_eq!(normalize_space(Some(" Work ".into())), Some("Work".into()));
    }

    #[test]
    fn basename_handles_trailing_slash_and_root() {
        assert_eq!(basename("/Users/y/proj"), "proj");
        assert_eq!(basename("/Users/y/proj/"), "proj");
        assert_eq!(basename("/"), "/");
    }
}

/// One directory the project browser can step into.
///
/// Absolute paths, unlike the Files view's [`crate::files::DirEntry`]: that one
/// is a node in a tree rooted at a session's directory, where this walks from
/// wherever the reader is and is handed straight to [`add_project`].
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "events.ts")]
#[serde(rename_all = "camelCase")]
pub struct FolderEntry {
    pub name: String,
    pub path: String,
    /// Whether the directory holds a `.git`, so the row that is almost always
    /// the one being looked for says so. A worktree's is a file rather than a
    /// directory, so presence is the test and not its kind.
    pub is_repo: bool,
}

/// One listing, plus the two things a browser needs beside it.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "events.ts")]
#[serde(rename_all = "camelCase")]
pub struct FolderListing {
    /// Canonical, so the path shown is the path attached — `add_project`
    /// canonicalizes too, and two spellings of one directory is how the picker
    /// ends up drawing "Attach project" over a project it just attached.
    pub path: String,
    /// `None` at the filesystem root, which is what removes the up row.
    pub parent: Option<String>,
    pub entries: Vec<FolderEntry>,
}

/// Directories under `path`, for picking a project on a machine the reader is
/// not sitting at.
///
/// The phone cannot use a native folder picker for two separate reasons, and
/// only the first is Android's: its dialog plugin implements none. The second
/// survives that being fixed — a picker there chooses a directory on the
/// *phone*, where a project has to be one on the machine the agent runs on. So
/// the browsing happens over the transport, against this.
///
/// An empty `path` means the reader's home directory, which is both the
/// sensible start and the only way the phone can learn where that is.
///
/// Directories alone: this picks a project, so a file is not a candidate and
/// listing one is a row that cannot be pressed. Dot-directories go with them —
/// `~` holds dozens and a project is not kept in one.
#[tauri::command]
pub async fn list_folders(path: String) -> Result<FolderListing, String> {
    let target = if path.is_empty() {
        std::env::home_dir().ok_or_else(|| "no home directory".to_string())?
    } else {
        std::path::PathBuf::from(&path)
    };

    // Before the read, so a path that resolves elsewhere is listed under the
    // name it actually has — a symlink stepped into otherwise reports the link
    // as its own location and the up row then climbs the wrong tree.
    let target = tokio::fs::canonicalize(&target)
        .await
        .map_err(|e| format!("{}: {e}", target.display()))?;

    let mut reader = tokio::fs::read_dir(&target)
        .await
        .map_err(|e| format!("{}: {e}", target.display()))?;

    let mut entries = Vec::new();
    while let Some(entry) = reader.next_entry().await.map_err(|e| e.to_string())? {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }

        // The link's target, so a symlinked directory is offered like any
        // other and a broken one is simply absent.
        let full = entry.path();
        if !tokio::fs::metadata(&full)
            .await
            .map(|meta| meta.is_dir())
            .unwrap_or(false)
        {
            continue;
        }

        entries.push(FolderEntry {
            name,
            is_repo: tokio::fs::symlink_metadata(full.join(".git")).await.is_ok(),
            path: full.to_string_lossy().into_owned(),
        });
    }

    entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.name.cmp(&b.name)));

    Ok(FolderListing {
        parent: target
            .parent()
            .map(|p| p.to_string_lossy().into_owned()),
        path: target.to_string_lossy().into_owned(),
        entries,
    })
}

#[cfg(test)]
mod folder_tests {
    use super::*;

    /// A directory of our own under the system temp dir. No `tempfile` here,
    /// which is not a dependency; the name carries a uuid so two runs cannot
    /// collide.
    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("dray-folders-{tag}-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    /// Home is what an empty path means, and it is canonical — the phone has no
    /// other way to learn where the reader's home is.
    #[tokio::test]
    async fn an_empty_path_lists_home() {
        let listing = list_folders(String::new()).await.expect("home lists");
        let home = tokio::fs::canonicalize(std::env::home_dir().unwrap())
            .await
            .unwrap();

        assert_eq!(listing.path, home.to_string_lossy());
        assert!(listing.parent.is_some(), "home is not the filesystem root");
    }

    /// Files are not candidates and dot-directories are not where projects
    /// live, so neither is drawn.
    #[tokio::test]
    async fn only_visible_directories_are_offered() {
        let dir = scratch("visible");
        tokio::fs::create_dir(dir.join("work")).await.unwrap();
        tokio::fs::create_dir(dir.join(".cache")).await.unwrap();
        tokio::fs::write(dir.join("notes.md"), "x").await.unwrap();

        let listing = list_folders(dir.to_string_lossy().into_owned())
            .await
            .expect("lists");

        let names: Vec<_> = listing.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["work"]);
        assert!(!listing.entries[0].is_repo);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The mark is `.git` existing, whatever it is: a linked worktree's is a
    /// file, so testing for a directory would leave every worktree unmarked.
    #[tokio::test]
    async fn a_repo_is_marked_whether_its_git_is_a_dir_or_a_file() {
        let dir = scratch("repos");
        tokio::fs::create_dir_all(dir.join("cloned/.git")).await.unwrap();
        tokio::fs::create_dir(dir.join("tree")).await.unwrap();
        tokio::fs::write(dir.join("tree/.git"), "gitdir: /elsewhere")
            .await
            .unwrap();

        let listing = list_folders(dir.to_string_lossy().into_owned())
            .await
            .expect("lists");

        assert_eq!(listing.entries.len(), 2);
        assert!(listing.entries.iter().all(|e| e.is_repo));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A directory that is not there answers with the reason rather than an
    /// empty list, which the picker would draw as a folder holding nothing.
    #[tokio::test]
    async fn a_missing_directory_is_an_error_not_an_empty_listing() {
        assert!(list_folders("/nope/not/here".to_string()).await.is_err());
    }
}
