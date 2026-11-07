# SPDX-License-Identifier: GPL-3.0-or-later
"""Sekka's adapter over the pinned Asahi installer.

The engine inspects a Mac and carries out one installation plan, speaking the
`sekka.mac.engine/1` transcript that `sekka-mac-contract` defines
(installer/genkan/macos/crates/contract/src/engine.rs). It imports Asahi's
modules from the extracted, pinned installer at run time; none of Asahi's
source is copied here.

    python3.13 -I -B ROOT/sekka_engine/__main__.py inspect
    python3.13 -I -B ROOT/sekka_engine/__main__.py execute --plan FILE \
        --assets DIR --installation UUID --owner NAME   (password on fd 3)

(`python3 -I -m sekka_engine …` where the package is importable.)

docs/apple-silicon/engine.md describes the modes, stages and evidence.
"""

# The engine's own version: the Asahi installer it adapts, and Sekka's
# revision of the adapter.
ENGINE_VERSION = "0.9.2+sekka.1"
ASAHI_VERSION = "0.9.2"
ASAHI_TAG = "v0.9.2"
PROTOCOL = 1
