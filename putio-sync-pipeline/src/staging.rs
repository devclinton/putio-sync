//! Staging (feature 038, R6): placeholder until the staging story.

use crate::store::ItemRow;
use crate::Pipeline;

impl Pipeline {
    /// Move a staged item to its final place; nothing to do when it was not staged
    pub(crate) async fn finish_staging(&self, _item: &ItemRow) -> Result<(), String> {
        Ok(())
    }
}
