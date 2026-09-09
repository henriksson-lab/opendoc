use crate::{AppApiError, AppDocument, OpenDocApp};
#[cfg(feature = "opendal-store")]
use opendoc_store::OpenDalObjectStore;
use opendoc_store::{FlatObjectStore, LocalObjectStore, ObjectStore, Repository};
use std::path::PathBuf;

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
            Self::Local => Ok(Repository::new(
                Box::new(LocalObjectStore::new(root)) as Box<dyn ObjectStore>
            )),
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
