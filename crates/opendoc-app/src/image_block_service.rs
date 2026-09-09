use crate::AppApiError;
use opendoc_core::{Block, BlockKind, HashRef, StableId};

pub(crate) struct ImageBlockService<'a> {
    blobs: &'a [crate::AppBlobRef],
}

impl<'a> ImageBlockService<'a> {
    pub(crate) fn new(blobs: &'a [crate::AppBlobRef]) -> Self {
        Self { blobs }
    }

    pub(crate) fn image_block_for_existing_blob(
        &self,
        blob_hash: impl AsRef<str>,
        alt_text: impl Into<String>,
    ) -> Result<Block, AppApiError> {
        let hash = HashRef::parse(blob_hash.as_ref().trim())
            .map_err(|err| AppApiError::Model(err.to_string()))?
            .to_string();
        if !self.blobs.iter().any(|blob| blob.hash == hash) {
            return Err(AppApiError::NotFound("blob was not found".to_string()));
        }
        Ok(Block {
            id: StableId::new("block"),
            kind: BlockKind::Image {
                blob_hash: hash,
                alt_text: alt_text.into(),
            },
            content: Vec::new(),
            properties: Vec::new(),
        })
    }
}
