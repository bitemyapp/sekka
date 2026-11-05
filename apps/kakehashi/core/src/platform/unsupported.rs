//! Systems Kakehashi does not write drives on.

use crate::drive::Drive;
use anyhow::{Result, bail};
use std::path::Path;

pub(crate) fn drives() -> Result<Vec<Drive>> {
    bail!("Kakehashi writes drives on Linux, macOS and Windows")
}

pub(crate) fn image_drive(_image: &Path) -> Option<String> {
    None
}
