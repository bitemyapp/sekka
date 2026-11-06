// SPDX-License-Identifier: GPL-3.0-or-later
//! Randomness for identifiers, from the operating system: `/dev/urandom` on
//! macOS and Linux alike.
use std::io::Read;

use crate::Error;

pub(crate) fn fill(bytes: &mut [u8]) -> Result<(), Error> {
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(bytes))
        .map_err(|e| Error::Invalid(format!("the system's random source failed: {e}")))
}
