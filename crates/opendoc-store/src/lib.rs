use opendoc_core::{digest_bytes, HashRef};
use opendoc_format::{decode_record, encode_record, BranchHeadRecord, ManifestRecord};
use std::fmt;
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

pub trait ObjectStore {
    fn put_if_absent(&self, hash: &HashRef, bytes: &[u8]) -> Result<bool, StoreError>;
    fn get(&self, hash: &HashRef) -> Result<Option<Vec<u8>>, StoreError>;
    fn exists(&self, hash: &HashRef) -> Result<bool, StoreError>;
    fn list_prefix(&self, prefix: &str) -> Result<Vec<String>, StoreError>;
    fn compare_and_swap_head(
        &self,
        document_uuid: &str,
        branch: &str,
        expected: Option<&HashRef>,
        new: &HashRef,
    ) -> Result<bool, StoreError>;
    fn read_head(&self, document_uuid: &str, branch: &str) -> Result<Option<HashRef>, StoreError>;
}

#[derive(Clone, Debug)]
pub struct Repository<S> {
    store: S,
}

impl<S: ObjectStore> Repository<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }

    pub fn store(&self) -> &S {
        &self.store
    }

    pub fn write_manifest(&self, manifest: &ManifestRecord) -> Result<HashRef, StoreError> {
        let bytes = encode_record(manifest);
        let hash = digest_bytes("sha256", &bytes).map_err(|_| StoreError::UnsupportedHash)?;
        self.store.put_if_absent(&hash, &bytes)?;
        Ok(hash)
    }

    pub fn read_manifest(&self, hash: &HashRef) -> Result<Option<ManifestRecord>, StoreError> {
        let Some(bytes) = self.store.get(hash)? else {
            return Ok(None);
        };
        let actual =
            digest_bytes(hash.algorithm(), &bytes).map_err(|_| StoreError::UnsupportedHash)?;
        if &actual != hash {
            return Err(StoreError::HashMismatch);
        }
        decode_record(&bytes)
            .map(Some)
            .map_err(|err| StoreError::Format(err.to_string()))
    }

    pub fn commit_manifest(
        &self,
        manifest: &ManifestRecord,
        expected: Option<&HashRef>,
    ) -> Result<Option<HashRef>, StoreError> {
        let manifest_hash = self.write_manifest(manifest)?;
        let head = BranchHeadRecord {
            document_uuid: manifest.document_uuid.clone(),
            branch: manifest.branch.clone(),
            manifest: manifest_hash.clone(),
        };
        let head_hash = digest_bytes("sha256", &encode_record(&head))
            .map_err(|_| StoreError::UnsupportedHash)?;
        self.store
            .put_if_absent(&head_hash, &encode_record(&head))?;
        if self.store.compare_and_swap_head(
            &manifest.document_uuid,
            &manifest.branch,
            expected,
            &manifest_hash,
        )? {
            Ok(Some(manifest_hash))
        } else {
            Ok(None)
        }
    }
}

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

    fn object_path(&self, hash: &HashRef) -> PathBuf {
        let digest = hash.digest();
        let prefix = &digest[..digest.len().min(2)];
        self.root
            .join("objects")
            .join(hash.algorithm())
            .join(prefix)
            .join(digest)
    }

    fn head_path(&self, document_uuid: &str, branch: &str) -> PathBuf {
        self.root
            .join("documents")
            .join(document_uuid)
            .join("heads")
            .join(format!("{branch}.head"))
    }
}

impl ObjectStore for LocalObjectStore {
    fn put_if_absent(&self, hash: &HashRef, bytes: &[u8]) -> Result<bool, StoreError> {
        let path = self.object_path(hash);
        if path.exists() {
            return Ok(false);
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
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
        let path = self.object_path(hash);
        match fs::read(path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
            Err(err) => Err(StoreError::Io(err.to_string())),
        }
    }

    fn exists(&self, hash: &HashRef) -> Result<bool, StoreError> {
        Ok(self.object_path(hash).exists())
    }

    fn list_prefix(&self, prefix: &str) -> Result<Vec<String>, StoreError> {
        let base = self.root.join(prefix);
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
        let current = self.read_head(document_uuid, branch)?;
        if current.as_ref() != expected {
            return Ok(false);
        }
        let path = self.head_path(document_uuid, branch);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
        fs::write(&tmp, new.to_string())?;
        fs::rename(tmp, path)?;
        Ok(true)
    }

    fn read_head(&self, document_uuid: &str, branch: &str) -> Result<Option<HashRef>, StoreError> {
        let path = self.head_path(document_uuid, branch);
        match fs::read_to_string(path) {
            Ok(value) => Ok(Some(
                HashRef::parse(value.trim()).map_err(|_| StoreError::CorruptHead)?,
            )),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
            Err(err) => Err(StoreError::Io(err.to_string())),
        }
    }
}

fn collect_paths(base: &Path, current: &Path, out: &mut Vec<String>) -> Result<(), StoreError> {
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

#[derive(Debug)]
pub enum StoreError {
    Io(String),
    CorruptHead,
    Format(String),
    HashMismatch,
    UnsupportedHash,
}

impl From<std::io::Error> for StoreError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value.to_string())
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for StoreError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_store_round_trips_object_and_head() {
        let root = std::env::temp_dir().join(format!("opendoc-store-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let store = LocalObjectStore::new(&root);
        let hash = HashRef::parse("sha256:abcdef").unwrap();
        assert!(store.put_if_absent(&hash, b"hello").unwrap());
        assert!(!store.put_if_absent(&hash, b"hello").unwrap());
        assert_eq!(store.get(&hash).unwrap(), Some(b"hello".to_vec()));
        assert!(store
            .compare_and_swap_head("doc", "main", None, &hash)
            .unwrap());
        assert_eq!(store.read_head("doc", "main").unwrap(), Some(hash.clone()));
        let other = HashRef::parse("sha256:123456").unwrap();
        assert!(!store
            .compare_and_swap_head("doc", "main", None, &other)
            .unwrap());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn repository_commits_and_reads_manifest() {
        let root = std::env::temp_dir().join(format!("opendoc-repo-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let repo = Repository::new(LocalObjectStore::new(&root));
        let manifest = ManifestRecord {
            document_uuid: "doc".to_string(),
            branch: "main".to_string(),
            parent: None,
            snapshot: HashRef::parse("sha256:aaa").unwrap(),
            operation_segments: Vec::new(),
            signatures: Vec::new(),
            blobs: Vec::new(),
            created_at_ms: 1,
        };
        let hash = repo.commit_manifest(&manifest, None).unwrap().unwrap();
        assert_eq!(repo.read_manifest(&hash).unwrap(), Some(manifest));
        assert_eq!(repo.store().read_head("doc", "main").unwrap(), Some(hash));
        let _ = fs::remove_dir_all(root);
    }
}
