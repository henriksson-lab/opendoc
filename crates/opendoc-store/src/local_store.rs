//! The on-disk object store: loose objects, head locking and directory walks.

use crate::error::StoreError;
use crate::keys::clean_relative_path;
use crate::object_store::{ObjectStore, ObjectStoreLayout, StoreCapabilities};
use crate::pack::{
    clean_pack_name, get_packed_from_dir, read_objects_from_pack_dir, write_pack_files, PackStats,
};
use opendoc_core::{digest_bytes, HashRef};
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct LocalObjectStore {
    root: PathBuf,
}

impl LocalObjectStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn object_path(&self, hash: &HashRef) -> PathBuf {
        self.root.join(ObjectStoreLayout::object_key(hash))
    }

    pub(crate) fn head_path(&self, document_uuid: &str, branch: &str) -> PathBuf {
        self.root.join(
            ObjectStoreLayout::head_key(document_uuid, branch)
                .expect("document uuid and branch are valid relative key segments"),
        )
    }

    pub fn compact_loose_objects_to_pack(&self, pack_name: &str) -> Result<PackStats, StoreError> {
        let pack_name = clean_pack_name(pack_name)?;
        let loose = self.loose_objects()?;
        let mut objects = self.objects_in_pack(pack_name)?;
        let mut packed_loose = Vec::new();
        for hash in loose {
            let Some(bytes) = self.get_loose(&hash)? else {
                continue;
            };
            packed_loose.push((hash.clone(), bytes.clone()));
            objects.push((hash, bytes));
        }
        let stats = self.write_pack(pack_name, &objects)?;
        // Only reclaim a loose object once it is readable from the pack.
        for (hash, bytes) in packed_loose {
            if self.get_packed(&hash)?.as_deref() == Some(bytes.as_slice()) {
                remove_file_if_exists(&self.object_path(&hash))?;
            }
        }
        Ok(stats)
    }

    pub fn write_pack(
        &self,
        pack_name: &str,
        objects: &[(HashRef, Vec<u8>)],
    ) -> Result<PackStats, StoreError> {
        let pack_name = clean_pack_name(pack_name)?;
        write_pack_files(&self.root.join("packs"), pack_name, objects)
    }

    fn objects_in_pack(&self, pack_name: &str) -> Result<Vec<(HashRef, Vec<u8>)>, StoreError> {
        let pack_name = clean_pack_name(pack_name)?;
        read_objects_from_pack_dir(&self.root.join("packs"), pack_name)
    }

    fn get_loose(&self, hash: &HashRef) -> Result<Option<Vec<u8>>, StoreError> {
        let path = self.object_path(hash);
        match fs::read(path) {
            Ok(bytes) => {
                let actual = digest_bytes(hash.algorithm(), &bytes)
                    .map_err(|_| StoreError::UnsupportedHash)?;
                if &actual != hash {
                    return Err(StoreError::HashMismatch);
                }
                Ok(Some(bytes))
            }
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
            Err(err) => Err(StoreError::Io(err.to_string())),
        }
    }

    fn get_packed(&self, hash: &HashRef) -> Result<Option<Vec<u8>>, StoreError> {
        get_packed_from_dir(&self.root.join("packs"), hash)
    }

    fn loose_objects(&self) -> Result<Vec<HashRef>, StoreError> {
        loose_objects_from_root(&self.root.join("objects"))
    }
}

pub(crate) fn loose_objects_from_root(object_root: &Path) -> Result<Vec<HashRef>, StoreError> {
    let mut out = Vec::new();
    if !object_root.exists() {
        return Ok(out);
    }
    for algorithm_entry in fs::read_dir(object_root)? {
        let algorithm_entry = algorithm_entry?;
        if !algorithm_entry.path().is_dir() {
            continue;
        }
        let algorithm = algorithm_entry.file_name().to_string_lossy().to_string();
        for prefix_entry in fs::read_dir(algorithm_entry.path())? {
            let prefix_entry = prefix_entry?;
            if !prefix_entry.path().is_dir() {
                continue;
            }
            for object_entry in fs::read_dir(prefix_entry.path())? {
                let object_entry = object_entry?;
                let path = object_entry.path();
                if !path.is_file() || path.extension().is_some() {
                    continue;
                }
                let digest = object_entry.file_name().to_string_lossy().to_string();
                let hash_text = format!("{algorithm}:{digest}");
                out.push(HashRef::parse(&hash_text).map_err(|_| StoreError::UnsupportedHash)?);
            }
        }
    }
    out.sort_by_key(|hash| hash.to_string());
    Ok(out)
}

impl ObjectStore for LocalObjectStore {
    fn capabilities(&self) -> StoreCapabilities {
        StoreCapabilities {
            idempotent_content_put: true,
            compare_and_swap_head: true,
            list_prefix: true,
            atomic_named_overwrite: true,
            local_pack_files: true,
        }
    }

    fn put_if_absent(&self, hash: &HashRef, bytes: &[u8]) -> Result<bool, StoreError> {
        let actual =
            digest_bytes(hash.algorithm(), bytes).map_err(|_| StoreError::UnsupportedHash)?;
        if &actual != hash {
            return Err(StoreError::HashMismatch);
        }
        let path = self.object_path(hash);
        if self.exists(hash)? {
            return Ok(false);
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension(format!("tmp-{}", process_tag()));
        {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp)?;
            file.write_all(bytes)?;
            file.sync_all()?;
        }
        match fs::hard_link(&tmp, &path) {
            Ok(()) => {
                let _ = fs::remove_file(&tmp);
                Ok(true)
            }
            Err(err) if err.kind() == ErrorKind::AlreadyExists => {
                let _ = fs::remove_file(&tmp);
                Ok(false)
            }
            Err(_) => {
                fs::rename(&tmp, &path)?;
                Ok(true)
            }
        }
    }

    fn get(&self, hash: &HashRef) -> Result<Option<Vec<u8>>, StoreError> {
        if let Some(bytes) = self.get_loose(hash)? {
            return Ok(Some(bytes));
        }
        self.get_packed(hash)
    }

    fn exists(&self, hash: &HashRef) -> Result<bool, StoreError> {
        if self.get_loose(hash)?.is_some() {
            return Ok(true);
        }
        Ok(self.get_packed(hash)?.is_some())
    }

    fn compact_loose_objects_to_pack(&self, pack_name: &str) -> Result<PackStats, StoreError> {
        LocalObjectStore::compact_loose_objects_to_pack(self, pack_name)
    }

    fn put_named(&self, path: &str, bytes: &[u8]) -> Result<(), StoreError> {
        let path = self.root.join(clean_relative_path(path)?);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension(format!("tmp-{}", process_tag()));
        {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&tmp)?;
            file.write_all(bytes)?;
            file.sync_all()?;
        }
        fs::rename(tmp, path)?;
        Ok(())
    }

    fn get_named(&self, path: &str) -> Result<Option<Vec<u8>>, StoreError> {
        let path = self.root.join(clean_relative_path(path)?);
        match fs::read(path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
            Err(err) => Err(StoreError::Io(err.to_string())),
        }
    }

    fn list_prefix(&self, prefix: &str) -> Result<Vec<String>, StoreError> {
        let base = self.root.join(clean_relative_path(prefix)?);
        let mut out = Vec::new();
        if !base.exists() {
            return Ok(out);
        }
        collect_paths(&base, &base, &mut out)?;
        out.sort();
        Ok(out)
    }

    fn compare_and_swap_head(
        &self,
        document_uuid: &str,
        branch: &str,
        expected: Option<&HashRef>,
        new: &HashRef,
    ) -> Result<bool, StoreError> {
        let path = self.head_path(document_uuid, branch);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let _lock = HeadLock::acquire(&head_lock_path(&path))?;
        let current = self.read_head(document_uuid, branch)?;
        if current.as_ref() != expected {
            return Ok(false);
        }
        let tmp = path.with_extension(format!("tmp-{}", process_tag()));
        fs::write(&tmp, new.to_string())?;
        fs::rename(tmp, path)?;
        Ok(true)
    }

    fn read_head(&self, document_uuid: &str, branch: &str) -> Result<Option<HashRef>, StoreError> {
        let path = self.head_path(document_uuid, branch);
        match fs::read_to_string(path) {
            Ok(value) => Ok(Some(parse_head_value(&value)?)),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
            Err(err) => Err(StoreError::Io(err.to_string())),
        }
    }
}

/// Process id for temp-file names; browsers have no processes.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn process_tag() -> u32 {
    std::process::id()
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn process_tag() -> u32 {
    0
}

pub(crate) fn remove_file_if_exists(path: &Path) -> Result<(), StoreError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => Err(StoreError::Io(err.to_string())),
    }
}

pub(crate) fn head_lock_path(head_path: &Path) -> PathBuf {
    let mut name = head_path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    name.push_str(".lock");
    head_path.with_file_name(name)
}

/// Exclusive advisory lock around a branch-head compare-and-swap, taken with
/// `O_EXCL` so two processes on the same filesystem cannot both observe the
/// old head and both "win" the swap. Stale locks left by a crashed process
/// are reclaimed after [`HEAD_LOCK_STALE_MS`].
pub(crate) struct HeadLock {
    path: PathBuf,
}

pub(crate) const HEAD_LOCK_STALE_MS: u128 = 30_000;
pub(crate) const HEAD_LOCK_ATTEMPTS: u32 = 200;

impl HeadLock {
    pub(crate) fn acquire(path: &Path) -> Result<Self, StoreError> {
        for _ in 0..HEAD_LOCK_ATTEMPTS {
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
            {
                Ok(mut file) => {
                    let _ = file.write_all(process_tag().to_string().as_bytes());
                    return Ok(Self {
                        path: path.to_path_buf(),
                    });
                }
                Err(err) if err.kind() == ErrorKind::AlreadyExists => {
                    let stale = fs::metadata(path)
                        .and_then(|meta| meta.modified())
                        .ok()
                        .and_then(|modified| modified.elapsed().ok())
                        .map(|age| age.as_millis() > HEAD_LOCK_STALE_MS)
                        .unwrap_or(false);
                    if stale {
                        let _ = fs::remove_file(path);
                        continue;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Err(err) => return Err(StoreError::Io(err.to_string())),
            }
        }
        Err(StoreError::Io(format!(
            "branch head lock {} is busy",
            path.display()
        )))
    }
}

impl Drop for HeadLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub(crate) fn parse_head_value(value: &str) -> Result<HashRef, StoreError> {
    if value.trim() != value {
        return Err(StoreError::CorruptHead);
    }
    HashRef::parse(value).map_err(|_| StoreError::CorruptHead)
}

pub(crate) fn collect_paths(
    base: &Path,
    current: &Path,
    out: &mut Vec<String>,
) -> Result<(), StoreError> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_paths(base, &path, out)?;
        } else if let Ok(relative) = path.strip_prefix(base) {
            out.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}
