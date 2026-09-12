//! An in-memory flat object store, used by the browser runtime and tests.

use crate::error::StoreError;
use crate::keys::{clean_object_prefix, clean_relative_path};
use crate::local_store::{
    collect_paths, head_lock_path, loose_objects_from_root, parse_head_value, process_tag,
    remove_file_if_exists, HeadLock,
};
use crate::object_store::{ObjectStore, ObjectStoreLayout, StoreCapabilities};
use crate::pack::{
    clean_pack_name, get_packed_from_dir, read_objects_from_pack_dir, write_pack_files, PackStats,
};
use opendoc_core::{digest_bytes, HashRef};
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct FlatObjectStore {
    root: PathBuf,
    namespace: String,
}

impl FlatObjectStore {
    pub fn new(root: impl Into<PathBuf>, namespace: impl Into<String>) -> Result<Self, StoreError> {
        let namespace = namespace.into();
        let namespace = clean_object_prefix(&namespace)?;
        Ok(Self {
            root: root.into(),
            namespace,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    pub(crate) fn key_path(&self, key: &str) -> Result<PathBuf, StoreError> {
        let mut path = self.root.clone();
        if !self.namespace.is_empty() {
            path = path.join(clean_relative_path(&self.namespace)?);
        }
        Ok(path.join(clean_relative_path(key)?))
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
        for (hash, bytes) in packed_loose {
            if self.get_packed(&hash)?.as_deref() == Some(bytes.as_slice()) {
                remove_file_if_exists(&self.key_path(&ObjectStoreLayout::object_key(&hash))?)?;
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
        let pack_dir = self.key_path("packs")?;
        write_pack_files(&pack_dir, pack_name, objects)
    }

    fn objects_in_pack(&self, pack_name: &str) -> Result<Vec<(HashRef, Vec<u8>)>, StoreError> {
        let pack_name = clean_pack_name(pack_name)?;
        read_objects_from_pack_dir(&self.key_path("packs")?, pack_name)
    }

    fn get_loose(&self, hash: &HashRef) -> Result<Option<Vec<u8>>, StoreError> {
        match fs::read(self.key_path(&ObjectStoreLayout::object_key(hash))?) {
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
        get_packed_from_dir(&self.key_path("packs")?, hash)
    }

    fn loose_objects(&self) -> Result<Vec<HashRef>, StoreError> {
        loose_objects_from_root(&self.key_path("objects")?)
    }
}

impl ObjectStore for FlatObjectStore {
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
        if self.exists(hash)? {
            return Ok(false);
        }
        let path = self.key_path(&ObjectStoreLayout::object_key(hash))?;
        if path.exists() {
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
        FlatObjectStore::compact_loose_objects_to_pack(self, pack_name)
    }

    fn put_named(&self, path: &str, bytes: &[u8]) -> Result<(), StoreError> {
        let path = self.key_path(path)?;
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
        match fs::read(self.key_path(path)?) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
            Err(err) => Err(StoreError::Io(err.to_string())),
        }
    }

    fn list_prefix(&self, prefix: &str) -> Result<Vec<String>, StoreError> {
        let base = self.key_path(prefix)?;
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
        let head_key = ObjectStoreLayout::head_key(document_uuid, branch)?;
        let head_path = self.key_path(&head_key)?;
        if let Some(parent) = head_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let _lock = HeadLock::acquire(&head_lock_path(&head_path))?;
        let current = self.read_head(document_uuid, branch)?;
        if current.as_ref() != expected {
            return Ok(false);
        }
        self.put_named(&head_key, new.to_string().as_bytes())?;
        Ok(true)
    }

    fn read_head(&self, document_uuid: &str, branch: &str) -> Result<Option<HashRef>, StoreError> {
        let Some(bytes) = self.get_named(&ObjectStoreLayout::head_key(document_uuid, branch)?)?
        else {
            return Ok(None);
        };
        let value = String::from_utf8(bytes).map_err(|err| StoreError::Format(err.to_string()))?;
        Ok(Some(parse_head_value(&value)?))
    }
}
