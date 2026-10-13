//! Systems Kakehashi does not write drives on.

use super::Device;
use crate::drive::Drive;
use anyhow::{Result, bail};
use std::path::Path;

pub(crate) type Held = ();

pub(crate) fn drives() -> Result<Vec<Drive>> {
    bail!("Kakehashi writes drives on Linux, macOS and Windows")
}

pub(crate) fn image_drive(_image: &Path) -> Option<String> {
    None
}

pub(crate) fn unmount(_drive: &Drive) -> Result<()> {
    bail!("unsupported")
}

pub(crate) fn open(_drive: &Drive) -> Result<Device> {
    bail!("unsupported")
}

pub(crate) fn drop_cache(_device: &Device) {}

pub(crate) fn eject(_drive: &Drive) -> Result<()> {
    bail!("unsupported")
}
