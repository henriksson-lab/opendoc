use crate::{AppApiError, AppDocument, OpenDocApp, APP_DOCUMENT_FORMAT};
use opendoc_format::{decode_cbor, decode_record, SnapshotRecord};
#[cfg(not(target_arch = "wasm32"))]
use opendoc_store::LocalObjectStore;
#[cfg(feature = "opendal-store")]
use opendoc_store::OpenDalObjectStore;
use opendoc_store::{FlatObjectStore, ObjectStore, Repository};
use std::path::{Path, PathBuf};

/// The object store that stands for "local storage" in this runtime.
///
/// On a native build that is the filesystem, and `root` is a real directory.
/// A browser has no filesystem: `LocalObjectStore` compiles for WebAssembly
/// and then fails every call, which is why the browser build had no
/// persistence at all. There, the same root names a subtree of the browser
/// volume `opendoc-wasm` installs over IndexedDB (ADR 0008), so
/// `save_local_repository` means the same thing in both runtimes without a
/// second set of commands.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn local_object_store(root: &Path) -> Result<Box<dyn ObjectStore>, AppApiError> {
    Ok(Box::new(LocalObjectStore::new(root)) as Box<dyn ObjectStore>)
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn local_object_store(root: &Path) -> Result<Box<dyn ObjectStore>, AppApiError> {
    let volume = opendoc_store::browser_volume().ok_or_else(|| {
        AppApiError::Store("this runtime has no local storage installed".to_string())
    })?;
    // Repositories share the volume with the recovery journal, so they live
    // under a subtree of their own; `object_store` refuses a root that tries
    // to climb out of it.
    let store = volume
        .object_store(&format!("repositories/{}", root.to_string_lossy()))
        .map_err(|err| AppApiError::Store(err.to_string()))?;
    Ok(Box::new(store) as Box<dyn ObjectStore>)
}

/// Decode a stored snapshot object into the app document it wraps.
///
/// Snapshot objects are a binary envelope around canonical CBOR; older objects
/// are bare CBOR, which the fallback below still reads.
pub(crate) fn decode_app_snapshot_object(
    bytes: &[u8],
) -> Result<SnapshotRecord<AppDocument>, AppApiError> {
    if let Ok(envelope) = decode_record::<SnapshotRecord<Vec<u8>>>(bytes) {
        envelope
            .validate()
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        let source: AppDocument =
            decode_cbor(&envelope.source).map_err(|err| AppApiError::Format(err.to_string()))?;
        let snapshot = SnapshotRecord::new(envelope.document_uuid, envelope.source_format, source);
        snapshot
            .validate()
            .map_err(|err| AppApiError::Format(err.to_string()))?;
        return Ok(snapshot);
    }
    decode_cbor(bytes).map_err(|err| AppApiError::Format(err.to_string()))
}

/// Validate that a decoded snapshot is one this build understands.
pub(crate) fn require_app_snapshot_format(
    snapshot: &SnapshotRecord<AppDocument>,
) -> Result<(), AppApiError> {
    if snapshot.source_format != APP_DOCUMENT_FORMAT {
        return Err(AppApiError::Format(format!(
            "unsupported snapshot source format {}",
            snapshot.source_format
        )));
    }
    Ok(())
}

impl OpenDocApp {
    /// The repository this document was last opened from or saved to, resolved
    /// into an object store. Version browsing is always relative to it.
    pub(crate) fn current_repository(
        &self,
        action: &str,
    ) -> Result<(PathBuf, Repository<Box<dyn ObjectStore>>), AppApiError> {
        let root = self.repository_root.clone().ok_or_else(|| {
            AppApiError::Conflict(format!("{action} needs an opened or saved repository"))
        })?;
        let target = AppRepositoryTarget::from_backend(
            self.repository_backend.as_deref(),
            self.repository_namespace.clone(),
            action,
        )?;
        let repository = target.repository(&root)?;
        Ok((root, repository))
    }

    pub(crate) fn current_repository_target(
        &self,
        action: &str,
    ) -> Result<(PathBuf, AppRepositoryTarget), AppApiError> {
        let root = self.repository_root.clone().ok_or_else(|| {
            AppApiError::Conflict(format!("{action} needs an opened or saved repository"))
        })?;
        let target = AppRepositoryTarget::from_backend(
            self.repository_backend.as_deref(),
            self.repository_namespace.clone(),
            action,
        )?;
        Ok((root, target))
    }
}

#[derive(Clone, Debug)]
pub(crate) enum AppRepositoryTarget {
    Local,
    Flat {
        namespace: String,
    },
    #[cfg(feature = "opendal-store")]
    OpenDalFs {
        namespace: String,
    },
}

impl AppRepositoryTarget {
    pub(crate) fn from_backend(
        backend: Option<&str>,
        namespace: Option<String>,
        action: &str,
    ) -> Result<Self, AppApiError> {
        match backend {
            Some("local") => Ok(Self::Local),
            Some("flat") => Ok(Self::Flat {
                namespace: namespace.ok_or_else(|| {
                    AppApiError::Conflict(format!("flat {action} needs a repository namespace"))
                })?,
            }),
            #[cfg(feature = "opendal-store")]
            Some("opendal-fs") => Ok(Self::OpenDalFs {
                namespace: namespace.ok_or_else(|| {
                    AppApiError::Conflict(format!(
                        "OpenDAL FS {action} needs a repository namespace"
                    ))
                })?,
            }),
            #[cfg(not(feature = "opendal-store"))]
            Some("opendal-fs") => Err(AppApiError::Conflict(
                "OpenDAL repository support is not enabled".to_string(),
            )),
            Some(other) => Err(AppApiError::Conflict(format!(
                "{action} does not support repository backend {other}"
            ))),
            None => Err(AppApiError::Conflict(format!(
                "{action} needs a repository backend"
            ))),
        }
    }

    pub(crate) fn repository(
        &self,
        root: &PathBuf,
    ) -> Result<Repository<Box<dyn ObjectStore>>, AppApiError> {
        match self {
            Self::Local => Ok(Repository::new(local_object_store(root)?)),
            Self::Flat { namespace } => Ok(Repository::new(Box::new(
                FlatObjectStore::new(root, namespace.clone())
                    .map_err(|err| AppApiError::Store(err.to_string()))?,
            ) as Box<dyn ObjectStore>)),
            #[cfg(feature = "opendal-store")]
            Self::OpenDalFs { namespace } => Ok(Repository::new(Box::new(
                OpenDalObjectStore::from_fs_root(root, namespace.clone())
                    .map_err(|err| AppApiError::Store(err.to_string()))?,
            )
                as Box<dyn ObjectStore>)),
        }
    }

    pub(crate) fn save(
        &self,
        app: &mut OpenDocApp,
        root: &PathBuf,
    ) -> Result<AppDocument, AppApiError> {
        match self {
            Self::Local => app.save_to_local_repository(root),
            Self::Flat { namespace } => app.save_to_flat_repository(root, namespace.clone()),
            #[cfg(feature = "opendal-store")]
            Self::OpenDalFs { namespace } => {
                app.save_to_opendal_fs_repository(root, namespace.clone())
            }
        }
    }

    pub(crate) fn open(
        &self,
        app: &mut OpenDocApp,
        root: PathBuf,
        document_uuid: String,
    ) -> Result<AppDocument, AppApiError> {
        match self {
            Self::Local => app.open_saved_projection(root, document_uuid),
            Self::Flat { namespace } => {
                app.open_flat_projection(root, namespace.clone(), document_uuid)
            }
            #[cfg(feature = "opendal-store")]
            Self::OpenDalFs { namespace } => {
                app.open_opendal_fs_projection(root, namespace.clone(), document_uuid)
            }
        }
    }
}
