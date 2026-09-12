//! The optional OpenDAL-backed object store (`opendal` feature).

use crate::error::StoreError;
use crate::keys::{clean_object_prefix, is_opendal_not_found, join_object_key, opendal_prefix_key};
use crate::local_store::parse_head_value;
use crate::object_store::{ObjectStore, ObjectStoreLayout, StoreCapabilities};
use opendoc_core::{digest_bytes, HashRef};
use std::path::Path;
#[cfg(feature = "opendal")]
use std::sync::Arc;

#[cfg(feature = "opendal")]
#[derive(Clone, Debug)]
pub struct OpenDalObjectStore {
    operator: opendal::blocking::Operator,
    namespace: String,
    _runtime: Option<Arc<tokio::runtime::Runtime>>,
}

#[cfg(feature = "opendal")]
impl OpenDalObjectStore {
    pub fn new(
        operator: opendal::blocking::Operator,
        namespace: impl Into<String>,
    ) -> Result<Self, StoreError> {
        let namespace = clean_object_prefix(&namespace.into())?;
        Ok(Self {
            operator,
            namespace,
            _runtime: None,
        })
    }

    pub fn from_fs_root(
        root: impl AsRef<Path>,
        namespace: impl Into<String>,
    ) -> Result<Self, StoreError> {
        let builder = opendal::services::Fs::default().root(
            root.as_ref()
                .to_str()
                .ok_or_else(|| StoreError::InvalidPath)?,
        );
        let operator =
            opendal::Operator::new(builder).map_err(|err| StoreError::Io(err.to_string()))?;
        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .map_err(|err| StoreError::Io(err.to_string()))?,
        );
        let _guard = runtime.enter();
        let operator = opendal::blocking::Operator::new(operator)
            .map_err(|err| StoreError::Io(err.to_string()))?;
        let mut store = Self::new(operator, namespace)?;
        store._runtime = Some(runtime);
        Ok(store)
    }

    pub fn operator(&self) -> &opendal::blocking::Operator {
        &self.operator
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    fn key(&self, key: &str) -> Result<String, StoreError> {
        join_object_key(&self.namespace, key)
    }
}

#[cfg(feature = "opendal")]
impl ObjectStore for OpenDalObjectStore {
    fn capabilities(&self) -> StoreCapabilities {
        StoreCapabilities {
            idempotent_content_put: true,
            compare_and_swap_head: false,
            list_prefix: true,
            atomic_named_overwrite: false,
            local_pack_files: false,
        }
    }

    fn put_if_absent(&self, hash: &HashRef, bytes: &[u8]) -> Result<bool, StoreError> {
        let actual =
            digest_bytes(hash.algorithm(), bytes).map_err(|_| StoreError::UnsupportedHash)?;
        if &actual != hash {
            return Err(StoreError::HashMismatch);
        }
        let key = self.key(&ObjectStoreLayout::object_key(hash))?;
        if self.exists(hash)? {
            return Ok(false);
        }
        self.operator
            .write(&key, bytes.to_vec())
            .map_err(|err| StoreError::Io(err.to_string()))?;
        Ok(true)
    }

    fn get(&self, hash: &HashRef) -> Result<Option<Vec<u8>>, StoreError> {
        match self
            .operator
            .read(&self.key(&ObjectStoreLayout::object_key(hash))?)
        {
            Ok(bytes) => {
                let bytes = bytes.to_vec();
                let actual = digest_bytes(hash.algorithm(), &bytes)
                    .map_err(|_| StoreError::UnsupportedHash)?;
                if &actual != hash {
                    return Err(StoreError::HashMismatch);
                }
                Ok(Some(bytes))
            }
            Err(err) if is_opendal_not_found(&err) => Ok(None),
            Err(err) => Err(StoreError::Io(err.to_string())),
        }
    }

    fn exists(&self, hash: &HashRef) -> Result<bool, StoreError> {
        Ok(self.get(hash)?.is_some())
    }

    fn put_named(&self, path: &str, bytes: &[u8]) -> Result<(), StoreError> {
        self.operator
            .write(&self.key(path)?, bytes.to_vec())
            .map_err(|err| StoreError::Io(err.to_string()))?;
        Ok(())
    }

    fn get_named(&self, path: &str) -> Result<Option<Vec<u8>>, StoreError> {
        match self.operator.read(&self.key(path)?) {
            Ok(bytes) => Ok(Some(bytes.to_vec())),
            Err(err) if is_opendal_not_found(&err) => Ok(None),
            Err(err) => Err(StoreError::Io(err.to_string())),
        }
    }

    fn list_prefix(&self, prefix: &str) -> Result<Vec<String>, StoreError> {
        let clean_prefix = opendal_prefix_key(prefix)?;
        let key_prefix = join_object_key(&self.namespace, &clean_prefix)?;
        let mut entries = self
            .operator
            .list_options(
                &key_prefix,
                opendal::options::ListOptions {
                    recursive: true,
                    ..Default::default()
                },
            )
            .map_err(|err| StoreError::Io(err.to_string()))?;
        entries.sort_by(|left, right| left.path().cmp(right.path()));
        let mut out = Vec::new();
        for entry in entries {
            if entry.metadata().mode() == opendal::EntryMode::DIR {
                continue;
            }
            let path = entry.path();
            let Some(relative) = path.strip_prefix(&key_prefix) else {
                continue;
            };
            if !relative.is_empty() {
                out.push(relative.to_string());
            }
        }
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
        self.put_named(
            &ObjectStoreLayout::head_key(document_uuid, branch)?,
            new.to_string().as_bytes(),
        )?;
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
