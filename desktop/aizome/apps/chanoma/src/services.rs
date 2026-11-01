//! The streaming services Chanoma knows: each opens full screen in the
//! browser, signed in once by a grown-up and remembered in a browser
//! profile of its own.
//!
//! A service's tile is its colour on a square with its initial, and its
//! name; never its logo.

/// How a controller drives the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Drive {
    /// A television interface: the arrows move between things, the
    /// confirm button chooses (YouTube's).
    Tv,
    /// An ordinary page: the left stick moves a pointer, the confirm
    /// button clicks; the arrows still scroll and seek.
    Pointer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Service {
    pub id: &'static str,
    pub name: &'static str,
    /// What the tile says, where the name is long.
    pub short: &'static str,
    pub url: &'static str,
    /// The colour of its tile's square, and of the initial on it (0xRRGGBB).
    pub color: u32,
    pub ink: u32,
    /// Made for kids.
    pub kids: bool,
    /// Needs an account.
    pub account: bool,
    pub drive: Drive,
    /// Ask for its television interface (a TV's user agent).
    pub tv_agent: bool,
}

/// A browser on a television, as YouTube's TV interface recognises one.
pub const TV_AGENT: &str =
    "Mozilla/5.0 (SMART-TV; LINUX; Tizen 6.5) AppleWebKit/537.36 (KHTML, like Gecko) 85.0.4183.93/6.5 TV Safari/537.36";

const fn service(id: &'static str, name: &'static str, url: &'static str, color: u32, ink: u32) -> Service {
    Service {
        id,
        name,
        short: name,
        url,
        color,
        ink,
        kids: false,
        account: true,
        drive: Drive::Pointer,
        tv_agent: false,
    }
}

pub const CATALOG: &[Service] = &[
    Service {
        kids: true,
        account: false,
        short: "PBS KIDS",
        ..service("pbs-kids", "PBS KIDS Video", "https://pbskids.org/video", 0x2b6dd1, 0xffffff)
    },
    Service {
        kids: true,
        account: false,
        ..service("youtube-kids", "YouTube Kids", "https://www.youtubekids.com", 0xff2b2b, 0xffffff)
    },
    Service {
        account: false,
        drive: Drive::Tv,
        tv_agent: true,
        ..service("youtube", "YouTube", "https://www.youtube.com/tv", 0x202020, 0xffffff)
    },
    service("netflix", "Netflix", "https://www.netflix.com/browse", 0x141414, 0xe50914),
    service("disney-plus", "Disney+", "https://www.disneyplus.com/home", 0x0e1a47, 0xffffff),
    Service {
        short: "Prime Video",
        ..service("prime-video", "Prime Video", "https://www.primevideo.com", 0x0f171e, 0x1aa7ec)
    },
    service("max", "Max", "https://play.max.com", 0x002be7, 0xffffff),
    service("hulu", "Hulu", "https://www.hulu.com/hub/home", 0x1ce783, 0x0b0c0f),
    service("apple-tv", "Apple TV", "https://tv.apple.com", 0x000000, 0xffffff),
    service("paramount-plus", "Paramount+", "https://www.paramountplus.com", 0x0064ff, 0xffffff),
    service("peacock", "Peacock", "https://www.peacocktv.com/watch/home", 0x000000, 0xffffff),
    service("crunchyroll", "Crunchyroll", "https://www.crunchyroll.com", 0xf47521, 0xffffff),
    Service { kids: true, ..service("kanopy-kids", "Kanopy Kids", "https://www.kanopy.com/kids", 0x6a3fd8, 0xffffff) },
    service("plex", "Plex", "https://app.plex.tv", 0x1f2326, 0xe5a00d),
    service("spotify", "Spotify", "https://open.spotify.com", 0x121212, 0x1ed760),
];

pub fn by_id(id: &str) -> Option<&'static Service> {
    CATALOG.iter().find(|s| s.id == id)
}

/// A site a grown-up added, as a service: its tile's colour from its name.
pub fn custom(name: &str) -> (u32, u32) {
    const COLOURS: [u32; 6] = [0x3d5a98, 0x7a3d98, 0x2d8a6e, 0xa8552a, 0x8a2d4c, 0x2d6e8a];
    let h = name.bytes().fold(0u32, |a, b| a.wrapping_mul(31).wrapping_add(u32::from(b)));
    (COLOURS[h as usize % COLOURS.len()], 0xffffff)
}

/// Whether `url` is an address a grown-up may add: http or https.
pub fn valid_url(url: &str) -> bool {
    let url = url.trim();
    (url.starts_with("https://") || url.starts_with("http://")) && url.len() > "https://x".len() && !url.contains(' ')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalogue_is_unique_and_the_defaults_are_in_it() {
        let mut ids: Vec<&str> = CATALOG.iter().map(|s| s.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), CATALOG.len());
        for id in crate::rules::Settings::default().services {
            assert!(by_id(&id).is_some_and(|s| s.kids && !s.account), "{id}");
        }
        assert!(CATALOG.iter().all(|s| s.url.starts_with("https://")));
    }

    #[test]
    fn grown_ups_add_web_addresses() {
        assert!(valid_url("https://jellyfin.home.arpa:8096"));
        assert!(!valid_url("file:///etc/passwd"));
        assert!(!valid_url("javascript:alert(1)"));
        assert!(!valid_url("https://"));
    }
}
