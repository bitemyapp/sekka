// SPDX-License-Identifier: GPL-3.0-or-later
//! Print the development key's public half as keys/development.pub.rs holds it.
fn main() {
    let key = sekka_mac_trust::signing::key(&sekka_mac_trust::signing::development_seed());
    print!("{}", sekka_mac_trust::signing::public_key_literal(&key));
}
