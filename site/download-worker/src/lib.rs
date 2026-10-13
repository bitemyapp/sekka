//! sekka.computer's one piece of server-side code: Cloudflare Pages runs it
//! for /download/ alone (_routes.json), where it lists the nightly builds in
//! the static page's place for them. Browsers get no script.
//!
//! The list comes from downloads.sekka.computer/nightly/builds.json through
//! Cloudflare's cache, which keeps it for shared::BUILDS_TTL seconds: R2
//! sees at most one request per data centre in that time. If it cannot be
//! had, the page keeps its fallback.

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
mod render;
#[allow(dead_code)]
#[path = "../../src/shared.rs"]
mod shared;

#[cfg(target_arch = "wasm32")]
mod handler {
    use crate::{render, shared};
    use worker::{
        CfProperties, Context, Date, Env, Fetch, Request, RequestInit, Response, ResponseBuilder,
        Result, event,
    };

    #[event(fetch)]
    async fn fetch(request: Request, env: Env, _ctx: Context) -> Result<Response> {
        let mut page = env.assets("ASSETS")?.fetch_request(request).await?;
        let html = page
            .headers()
            .get("content-type")?
            .is_some_and(|t| t.starts_with("text/html"));
        if page.status_code() != 200 || !html {
            return Ok(page);
        }
        // NIGHTLY_URL stands in for downloads.sekka.computer in local tests.
        let base = env
            .var("NIGHTLY_URL")
            .map(|v| v.to_string())
            .unwrap_or_else(|_| shared::NIGHTLY.to_owned());
        let mut body = page.text().await?;
        if let Some(list) = list(&base).await {
            if let Some(filled) = render::fill(&body, &list) {
                body = filled;
            }
        }
        let headers = page.headers().clone();
        headers.delete("content-length")?;
        headers.delete("etag")?;
        for (name, value) in shared::HEADERS {
            headers.set(name, value)?;
        }
        headers.set("cache-control", "no-cache")?;
        ResponseBuilder::new().with_headers(headers).from_html(body)
    }

    /// The builds as HTML, a note that there are none yet, or None when the
    /// list cannot be had and the page's fallback should stay.
    async fn list(base: &str) -> Option<String> {
        let mut init = RequestInit::new();
        init.with_cf_properties(CfProperties {
            cache_everything: Some(true),
            cache_ttl: Some(shared::BUILDS_TTL as i32),
            ..CfProperties::default()
        });
        let request = Request::new_with_init(&format!("{base}/builds.json"), &init).ok()?;
        let mut response = Fetch::Request(request).send().await.ok()?;
        match response.status_code() {
            404 => Some(render::NONE_YET.to_owned()),
            200 => {
                let builds: render::Builds = response.json().await.ok()?;
                if builds.builds.is_empty() {
                    return None;
                }
                let now = (Date::now().as_millis() / 1000) as i64;
                Some(render::render(&builds.builds, base, shared::REPO, now))
            }
            _ => None,
        }
    }
}
