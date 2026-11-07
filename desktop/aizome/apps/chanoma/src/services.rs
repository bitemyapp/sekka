//! The streaming services Chanoma knows: each opens full screen in the
//! browser, signed in once by a grown-up and remembered in a browser
//! profile of its own.
//!
//! A service's tile is its colour on a square with its initial, and its
//! name; never its logo.
//!
//! In kids' mode a service's pages stay on its [`Sites`]: its address's
//! site and those it needs besides (`crate::devtools` keeps them there).

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
    /// Sites its pages pass through besides its address's, as a browser
    /// opening the address goes.
    pub sites: &'static [&'static str],
    /// The colour of its tile's square, and of the initial on it (0xRRGGBB).
    pub color: u32,
    pub ink: u32,
    /// Made for kids.
    pub kids: bool,
    /// Needs an account.
    pub account: bool,
    /// Protects its shows (Widevine), which only Google Chrome plays here.
    pub drm: bool,
    pub drive: Drive,
    /// Ask for its television interface (a TV's user agent).
    pub tv_agent: bool,
    /// Chanoma's keyboard comes up for its text boxes. A television
    /// interface has its own, which the arrows work.
    pub keyboard: bool,
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
        sites: &[],
        color,
        ink,
        kids: false,
        account: true,
        // The paid services do, and Plex's and Kanopy's free films;
        // PBS KIDS and YouTube don't, below.
        drm: true,
        drive: Drive::Pointer,
        tv_agent: false,
        keyboard: true,
    }
}

pub const CATALOG: &[Service] = &[
    Service {
        kids: true,
        account: false,
        short: "PBS KIDS",
        drm: false,
        ..service("pbs-kids", "PBS KIDS Video", "https://pbskids.org/video", 0x2b6dd1, 0xffffff)
    },
    // Google's sign-in, which YouTube Kids passes through as it opens,
    // signed in or not (and Google's sites to stay signed in); not the
    // rest of Google.
    Service {
        kids: true,
        account: false,
        sites: &["accounts.google.com"],
        drm: false,
        ..service("youtube-kids", "YouTube Kids", "https://www.youtubekids.com", 0xff2b2b, 0xffffff)
    },
    Service {
        account: false,
        drm: false,
        drive: Drive::Tv,
        tv_agent: true,
        keyboard: false,
        sites: &["accounts.google.com"],
        ..service("youtube", "YouTube", "https://www.youtube.com/tv", 0x202020, 0xffffff)
    },
    service("netflix", "Netflix", "https://www.netflix.com/browse", 0x141414, 0xe50914),
    service("disney-plus", "Disney+", "https://www.disneyplus.com/home", 0x0e1a47, 0xffffff),
    Service {
        short: "Prime Video",
        ..service("prime-video", "Prime Video", "https://www.primevideo.com", 0x0f171e, 0x1aa7ec)
    },
    // Max is HBO Max again, where its address leads.
    Service { sites: &["max.com", "hbomax.com"], ..service("max", "Max", "https://play.max.com", 0x002be7, 0xffffff) },
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

/// Where a service's pages may go: sites, each with its subdomains
/// (`netflix.com` is `www.netflix.com` too, but not `netflix.com.example`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sites(Vec<String>);

impl Sites {
    /// The site of the address `url` (its host, less a `www.`), and `more`.
    pub fn new(url: &str, more: &[&str]) -> Sites {
        let own = host(url).map(|h| h.strip_prefix("www.").map(str::to_owned).unwrap_or(h));
        let mut sites: Vec<String> = own.into_iter().collect();
        for s in more.iter().map(|s| s.trim().trim_end_matches('.').to_ascii_lowercase()) {
            if !s.is_empty() && !sites.contains(&s) {
                sites.push(s);
            }
        }
        Sites(sites)
    }

    /// A service's: its address's site and those it passes through.
    pub fn of(service: &Service) -> Sites {
        Sites::new(service.url, service.sites)
    }

    /// Whether a page may be at `url`: on one of the sites, or on a page
    /// of the browser's own that leads nowhere (a blank one, or the one
    /// saying a site can't be reached).
    pub fn allows(&self, url: &str) -> bool {
        let Some((scheme, rest)) = url.split_once(':') else { return false };
        match scheme.to_ascii_lowercase().as_str() {
            "http" | "https" => host(url).is_some_and(|h| {
                self.0.iter().any(|s| h == *s || h.strip_suffix(s.as_str()).is_some_and(|sub| sub.ends_with('.')))
            }),
            // Made by a page, on the page's site.
            "blob" => self.allows(rest),
            "about" => matches!(rest.split(['?', '#']).next(), Some("blank" | "srcdoc")),
            "chrome-error" => true,
            _ => false,
        }
    }
}

impl std::fmt::Display for Sites {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0.join(", "))
    }
}

/// The host of the web address `url`, lower-case, without a user, a port
/// or a final dot: `https://kid@Example.com.:8443/x` is `example.com`.
pub fn host(url: &str) -> Option<String> {
    let (scheme, rest) = url.trim().split_once("://")?;
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return None;
    }
    let authority = rest.split(['/', '\\', '?', '#']).next()?;
    let at = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = match at.strip_prefix('[') {
        Some(v6) => v6.split(']').next()?,
        None => at.split(':').next()?,
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
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
        for s in CATALOG {
            assert!(Sites::of(s).allows(s.url), "{}", s.id);
            // Chanoma's keyboard where nothing else types: a television
            // interface has its own.
            assert_eq!(s.keyboard, s.drive == Drive::Pointer, "{}", s.id);
        }
    }

    #[test]
    fn a_page_stays_on_its_sites() {
        let s = Sites::of(by_id("youtube-kids").unwrap());
        assert_eq!(s.to_string(), "youtubekids.com, accounts.google.com");
        assert!(s.allows("https://www.youtubekids.com/watch?v=x"));
        assert!(s.allows("https://youtubekids.com/"));
        assert!(s.allows("https://accounts.google.com/ServiceLogin?passive=true"));
        assert!(s.allows("https://kid@www.youtubekids.com:443/"));
        assert!(s.allows("blob:https://www.youtubekids.com/0b5e"));
        assert!(s.allows("about:blank"));
        assert!(s.allows("chrome-error://chromewebdata/"));
        // The rest of Google, look-alikes, and what isn't the web.
        for url in [
            "https://www.google.com/search?q=x",
            "https://myaccount.google.com/",
            "https://youtubekids.com.example.net/",
            "https://notyoutubekids.com/",
            "https://youtubekids.com@example.net/",
            "https://example.net\\@youtubekids.com/",
            "blob:https://example.net/0b5e",
            "file:///etc/passwd",
            "data:text/html,hi",
            "chrome://settings",
            "view-source:https://www.youtubekids.com/",
            "javascript:alert(1)",
            "about:version",
        ] {
            assert!(!s.allows(url), "{url}");
        }
        // A site a grown-up added is its host's, port or not.
        let site = Sites::new("http://127.0.0.1:8000/", &[]);
        assert!(site.allows("http://127.0.0.1:8000/?inside"));
        assert!(site.allows("http://127.0.0.1:9000/"));
        assert!(!site.allows("http://localhost:8000/"));
        assert_eq!(host("https://kid@Example.COM.:8443/x?y#z").as_deref(), Some("example.com"));
        assert_eq!(host("http://[::1]:8000/").as_deref(), Some("::1"));
        assert_eq!(host("ftp://example.com/"), None);
    }

    #[test]
    fn grown_ups_add_web_addresses() {
        assert!(valid_url("https://jellyfin.home.arpa:8096"));
        assert!(!valid_url("file:///etc/passwd"));
        assert!(!valid_url("javascript:alert(1)"));
        assert!(!valid_url("https://"));
    }
}
