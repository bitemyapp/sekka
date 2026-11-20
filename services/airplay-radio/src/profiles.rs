//! Fixed diagnostic settings. No caller-supplied paths, commands or RF limits.
#[derive(Clone, Copy, Debug)]
pub struct Profile {
    pub monitor: bool,
    pub channel: u8,
    pub rate: &'static str,
    pub no_ack: bool,
    pub amsdu: bool,
    pub nearby_only: bool,
    pub echo: bool,
    pub mtu: u32,
}

pub const NAMES: &[&str] = &[
    "m6-12",
    "m6-24",
    "m6-54",
    "m6-mcs0",
    "m6-mcs3",
    "m6-mcs7",
    "m6-ack",
    "m6-amsdu",
    "m6-near",
    "m6-quiet",
    "m6-mtu1280",
    "m44-12",
    "m149-12",
    "ibss6",
    "ibss6-mcs7",
];

pub fn get(name: &str) -> Result<Profile, String> {
    let mut p = Profile {
        monitor: true,
        channel: 6,
        rate: "mcs7",
        no_ack: true,
        amsdu: false,
        nearby_only: false,
        echo: true,
        mtu: 1450,
    };
    match name {
        "m6-12" => p.rate = "12",
        "m6-24" => p.rate = "24",
        "m6-54" => p.rate = "54",
        "m6-mcs0" => p.rate = "mcs0",
        "m6-mcs3" => p.rate = "mcs3",
        "m6-mcs7" => {}
        "m6-ack" => p.no_ack = false,
        "m6-amsdu" => p.amsdu = true,
        "m6-near" => p.nearby_only = true,
        "m6-quiet" => p.echo = false,
        "m6-mtu1280" => p.mtu = 1280,
        "m44-12" => {
            p.channel = 44;
            p.rate = "12";
        }
        "m149-12" => {
            p.channel = 149;
            p.rate = "12";
        }
        "ibss6" => {
            p.monitor = false;
            p.rate = "12";
        }
        "ibss6-mcs7" => p.monitor = false,
        _ => return Err(format!("Unknown radio trial profile: {name}")),
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profiles_are_bounded_and_invalid_input_is_rejected() {
        for name in NAMES {
            let p = get(name).unwrap();
            assert!([6, 44, 149].contains(&p.channel));
            assert!((1280..=1450).contains(&p.mtu));
        }
        for invalid in ["", "m6-mcs16", "../custom", "m149-override"] {
            assert!(get(invalid).is_err());
        }
    }
}
