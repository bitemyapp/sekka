//! Systems Kakehashi does not write drives on.

use super::Device;
use anyhow::{Result, bail};
use kakehashi_core::Drive;

pub(crate) type Held = ();

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
