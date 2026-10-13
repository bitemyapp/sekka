//! What the site's pages and its download worker (site/download-worker)
//! share: where things are, and the headers every response carries.

pub const REPO: &str = "https://github.com/bitemyapp/sekka";

/// Nightly ISOs: the Cloudflare R2 bucket sekka-downloads (DEVELOPERS.md).
pub const NIGHTLY: &str = "https://downloads.sekka.computer/nightly";

/// How long Cloudflare's cache keeps the list of nightly builds for the
/// worker, in seconds: R2 sees at most one request per data centre in that
/// time, and a new build is listed within it.
pub const BUILDS_TTL: u32 = 300;

/// Every page's headers. _headers gives them to static files; the worker,
/// whose responses Pages leaves alone, sets them itself.
pub const HEADERS: &[(&str, &str)] = &[
    (
        "Content-Security-Policy",
        "default-src 'none'; style-src 'self'; img-src 'self'; font-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
    ),
    ("Referrer-Policy", "no-referrer"),
    ("X-Content-Type-Options", "nosniff"),
    (
        "Permissions-Policy",
        "camera=(), microphone=(), geolocation=(), browsing-topics=()",
    ),
    ("Cross-Origin-Opener-Policy", "same-origin"),
    ("Strict-Transport-Security", "max-age=31536000"),
];
