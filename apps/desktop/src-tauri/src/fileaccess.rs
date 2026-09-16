//! Reading and writing files, and the grants that bound which files.
//!
//! # Why a grant and not a path
//!
//! `read_file_base64` and `write_file_base64` used to take a path and act on
//! it. The Tauri access-control list has no scope mechanism for hand-rolled
//! commands — the generated permission says so in as many words, "without any
//! pre-configured scope" — so "the page may call this command" meant "the page
//! may read and write any file the user can". Any script that reached `invoke`
//! had the user's whole filesystem, which is what turns a rendering bug into a
//! local file read.
//!
//! The confinement therefore has to be *in* the command. What bounds it is the
//! only thing that legitimately does: the user picked that file, in a native
//! dialog, a moment ago. [`pick_open_path`](crate::pick_open_path) and
//! [`pick_save_path`](crate::pick_save_path) mint a one-shot grant for what the
//! user chose; these commands spend it. A path nobody picked is refused, a
//! grant is good for one call, and grants expire.
//!
//! The frontend is unchanged by this: it already picks and then immediately
//! reads or writes, and the dialogs still return the real path because the
//! rest of the app (window titles, the repository root) needs it.
//!
//! # What the grant is keyed on
//!
//! The canonical path, resolved through symlinks, at both ends. A grant minted
//! for `~/notes.txt` does not spend on `/etc/shadow` even if something swaps a
//! symlink in between — the two canonical paths differ and the call is
//! refused. The bytes are read from, and written to, the path stored in the
//! grant rather than the one the caller sent.

use std::collections::VecDeque;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use opendoc_app::{base64_decode, base64_encode};
use serde::Serialize;

/// Largest file this shell will read into memory.
///
/// The bytes become a base64 string and then a document; anything past this is
/// not an import, it is a way to exhaust memory from a file chooser.
const MAX_READ_BYTES: u64 = 256 * 1024 * 1024;

/// How long a grant is good for. Long enough that a slow export still lands,
/// short enough that a grant is not a standing capability.
const GRANT_LIFETIME: Duration = Duration::from_secs(300);

/// How many grants may be outstanding. A dialog the user cancels mints
/// nothing, so this is only ever reached by something minting without
/// spending.
const MAX_GRANTS: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Access {
    Read,
    Write,
}

struct Grant {
    path: PathBuf,
    access: Access,
    minted: Instant,
}

/// The outstanding grants. Managed state, so it lives as long as the shell.
#[derive(Default)]
pub(crate) struct FileGrants {
    grants: Mutex<VecDeque<Grant>>,
}

impl FileGrants {
    /// Record that the user chose `path` for `access`.
    ///
    /// Returns quietly on a path that cannot be canonicalised: the grant
    /// simply is not minted, and the command that would have spent it refuses
    /// with the same message as any unpicked path.
    pub(crate) fn mint(&self, path: &Path, access: Access) {
        let Some(canonical) = canonical_for(path, access) else {
            return;
        };
        let Ok(mut grants) = self.grants.lock() else {
            return;
        };
        let now = Instant::now();
        grants.retain(|grant| now.duration_since(grant.minted) < GRANT_LIFETIME);
        while grants.len() >= MAX_GRANTS {
            grants.pop_front();
        }
        grants.push_back(Grant {
            path: canonical,
            access,
            minted: now,
        });
    }

    /// Spend a grant for `requested`, and answer with the path to actually use.
    ///
    /// One shot: the grant is removed whether or not the call that spends it
    /// succeeds, so a refused write does not leave a standing permission.
    pub(crate) fn spend(&self, requested: &str, access: Access) -> Result<PathBuf, String> {
        let requested = PathBuf::from(requested);
        let canonical = canonical_for(&requested, access);
        let mut grants = self
            .grants
            .lock()
            .map_err(|_| "file access state is poisoned".to_string())?;
        let now = Instant::now();
        grants.retain(|grant| now.duration_since(grant.minted) < GRANT_LIFETIME);
        let found = canonical.as_ref().and_then(|canonical| {
            grants
                .iter()
                .position(|grant| grant.access == access && &grant.path == canonical)
        });
        match found {
            Some(index) => Ok(grants
                .remove(index)
                .expect("index came from this deque")
                .path),
            None => Err(format!(
                "{} was not chosen in a file dialog, so this shell will not {} it",
                requested.display(),
                match access {
                    Access::Read => "read",
                    Access::Write => "write",
                }
            )),
        }
    }
}

/// The path a grant is keyed on.
///
/// A file being read has to exist, so it canonicalises directly. A file being
/// written may not exist yet, so its *directory* is canonicalised and the name
/// rejoined — which is what stops `../` in the name from walking out of the
/// directory the user chose.
fn canonical_for(path: &Path, access: Access) -> Option<PathBuf> {
    match access {
        Access::Read => path.canonicalize().ok(),
        Access::Write => {
            let name = path.file_name()?;
            if Path::new(name).components().count() != 1 {
                return None;
            }
            let parent = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())?;
            Some(parent.canonicalize().ok()?.join(name))
        }
    }
}

#[derive(Serialize)]
pub(crate) struct FileContents {
    pub(crate) name: String,
    pub(crate) path: String,
    pub(crate) media_type: String,
    pub(crate) size: usize,
    pub(crate) base64: String,
}

pub(crate) fn media_type_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("bmp") => "image/bmp",
        Some("docx") => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        Some("doc") => "application/msword",
        Some("xlsx") => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        Some("csv") => "text/csv",
        Some("tsv") => "text/tab-separated-values",
        Some("json") => "application/json",
        Some("md") => "text/markdown",
        Some("html") | Some("htm") => "text/html",
        Some("txt") => "text/plain",
        Some("pdf") => "application/pdf",
        _ => "application/octet-stream",
    }
}

#[tauri::command]
pub(crate) fn read_file_base64(
    grants: tauri::State<'_, FileGrants>,
    path: String,
) -> Result<FileContents, String> {
    let path = grants.spend(&path, Access::Read)?;
    let file = std::fs::File::open(&path).map_err(|err| format!("{}: {err}", path.display()))?;
    let declared = file
        .metadata()
        .map_err(|err| format!("{}: {err}", path.display()))?
        .len();
    if declared > MAX_READ_BYTES {
        return Err(format!(
            "{} is {declared} bytes, over the {MAX_READ_BYTES}-byte limit",
            path.display()
        ));
    }
    // Read through a `take` rather than trusting the metadata: on a growing
    // file, or on anything that is not a plain file, the size is a hint.
    let mut bytes = Vec::new();
    file.take(MAX_READ_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|err| format!("{}: {err}", path.display()))?;
    if bytes.len() as u64 > MAX_READ_BYTES {
        return Err(format!(
            "{} is larger than the {MAX_READ_BYTES}-byte limit",
            path.display()
        ));
    }
    Ok(FileContents {
        name: path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default(),
        path: path.to_string_lossy().to_string(),
        media_type: media_type_for(&path).to_string(),
        size: bytes.len(),
        base64: base64_encode(&bytes),
    })
}

#[tauri::command]
pub(crate) fn write_file_base64(
    grants: tauri::State<'_, FileGrants>,
    path: String,
    base64: String,
) -> Result<(), String> {
    let path = grants.spend(&path, Access::Write)?;
    let bytes = base64_decode(&base64).ok_or_else(|| "invalid base64 payload".to_string())?;
    std::fs::write(&path, bytes).map_err(|err| format!("{}: {err}", path.display()))
}

#[tauri::command]
pub(crate) fn write_file_text(
    grants: tauri::State<'_, FileGrants>,
    path: String,
    text: String,
) -> Result<(), String> {
    let path = grants.spend(&path, Access::Write)?;
    std::fs::write(&path, text).map_err(|err| format!("{}: {err}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "opendoc-grant-{label}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    #[test]
    fn a_path_nobody_picked_is_refused() {
        let grants = FileGrants::default();
        let dir = scratch("unpicked");
        let secret = dir.join("secret");
        std::fs::write(&secret, b"x").unwrap();
        let err = grants
            .spend(secret.to_str().unwrap(), Access::Read)
            .unwrap_err();
        assert!(err.contains("was not chosen in a file dialog"), "{err}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_picked_path_spends_once_and_only_once() {
        let grants = FileGrants::default();
        let dir = scratch("once");
        let picked = dir.join("picked.txt");
        std::fs::write(&picked, b"x").unwrap();

        grants.mint(&picked, Access::Read);
        assert_eq!(
            grants
                .spend(picked.to_str().unwrap(), Access::Read)
                .unwrap(),
            picked.canonicalize().unwrap()
        );
        // Spent. A second call is a call nobody authorised.
        assert!(grants
            .spend(picked.to_str().unwrap(), Access::Read)
            .is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_read_grant_is_not_a_write_grant() {
        let grants = FileGrants::default();
        let dir = scratch("access");
        let picked = dir.join("picked.txt");
        std::fs::write(&picked, b"x").unwrap();

        grants.mint(&picked, Access::Read);
        assert!(grants
            .spend(picked.to_str().unwrap(), Access::Write)
            .is_err());
        assert!(grants.spend(picked.to_str().unwrap(), Access::Read).is_ok());

        grants.mint(&picked, Access::Write);
        assert!(grants
            .spend(picked.to_str().unwrap(), Access::Read)
            .is_err());
        assert!(grants
            .spend(picked.to_str().unwrap(), Access::Write)
            .is_ok());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The grant is on the file, not on the string. Spelling the same file
    /// differently still works; spelling a different file does not.
    #[test]
    fn grants_are_keyed_on_the_canonical_path() {
        let grants = FileGrants::default();
        let dir = scratch("canonical");
        let picked = dir.join("picked.txt");
        std::fs::write(&picked, b"x").unwrap();
        std::fs::write(dir.join("other.txt"), b"y").unwrap();

        grants.mint(&picked, Access::Read);
        let roundabout = dir
            .join("..")
            .join(dir.file_name().unwrap())
            .join("picked.txt");
        assert!(grants
            .spend(roundabout.to_str().unwrap(), Access::Read)
            .is_ok());

        grants.mint(&picked, Access::Read);
        let sibling = dir.join("other.txt");
        assert!(grants
            .spend(sibling.to_str().unwrap(), Access::Read)
            .is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A grant for a file to be written does not become a grant for its
    /// neighbours by way of `..`.
    #[test]
    fn a_write_grant_does_not_walk_out_of_its_directory() {
        let grants = FileGrants::default();
        let dir = scratch("write-escape");
        let inner = dir.join("inner");
        std::fs::create_dir_all(&inner).unwrap();
        let picked = inner.join("export.docx");

        grants.mint(&picked, Access::Write);
        let escape = inner.join("..").join("elsewhere.docx");
        assert!(grants
            .spend(escape.to_str().unwrap(), Access::Write)
            .is_err());
        assert!(grants
            .spend(picked.to_str().unwrap(), Access::Write)
            .is_ok());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A symlink pointing somewhere else is not the file the user chose, even
    /// when the string is the one the dialog returned.
    #[cfg(unix)]
    #[test]
    fn a_symlink_swapped_after_the_dialog_does_not_spend_the_grant() {
        let grants = FileGrants::default();
        let dir = scratch("symlink");
        let real = dir.join("real.txt");
        let secret = dir.join("secret.txt");
        std::fs::write(&real, b"x").unwrap();
        std::fs::write(&secret, b"s").unwrap();
        let link = dir.join("link.txt");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        grants.mint(&link, Access::Read);
        // The link now points at something else; the string has not changed.
        std::fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(&secret, &link).unwrap();
        assert!(grants.spend(link.to_str().unwrap(), Access::Read).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn media_types_come_from_the_extension() {
        assert_eq!(media_type_for(Path::new("a.png")), "image/png");
        assert_eq!(media_type_for(Path::new("a.PNG")), "image/png");
        assert_eq!(media_type_for(Path::new("a")), "application/octet-stream");
    }
}
