//! sekka.computer's one piece of server-side code: Cloudflare Pages runs it
//! for /download/ alone (_routes.json), where it lists the nightly builds in
//! the static page's place for them. Browsers get no script.
//!
//! The list comes from downloads.sekka.computer/nightly/builds.json, kept
//! in each Cloudflare data centre's cache (the Cache API) for
//! shared::BUILDS_TTL seconds: R2 sees at most one request per data centre
//! in that time. If it cannot be had, the page keeps its fallback.

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
mod render;
#[allow(dead_code)]
#[path = "../../src/shared.rs"]
mod shared;

#[cfg(target_arch = "wasm32")]
mod handler {
    use crate::{render, shared};
    use worker::{
        Cache, Context, Date, Env, Fetch, Headers, Method, Request, Response, ResponseBuilder,
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
        let (list, cache) = list(&base).await;
        if let Some(list) = list {
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
        // Whether the list came from the cache or R2, to see from outside.
        if let Some(cache) = cache {
            headers.set("x-builds-cache", cache)?;
        }
        ResponseBuilder::new().with_headers(headers).from_html(body)
    }

    /// The builds as HTML (a note that there are none yet, or None when the
    /// list cannot be had and the page's fallback should stay), and where the
    /// list came from: HIT for this data centre's cache, MISS for R2.
    async fn list(base: &str) -> (Option<String>, Option<&'static str>) {
        let url = format!("{base}/builds.json");
        let now = (Date::now().as_millis() / 1000) as i64;
        let cache = Cache::default();
        if let Ok(Some(mut cached)) = cache.get(&url, true).await {
            let status = cached.status_code();
            if let Ok(body) = cached.text().await {
                return (
                    render::response(status, &body, base, shared::REPO, now),
                    Some("HIT"),
                );
            }
        }
        let Ok(request) = Request::new(&url, Method::Get) else {
            return (None, None);
        };
        let Ok(mut response) = Fetch::Request(request).send().await else {
            return (None, None);
        };
        let status = response.status_code();
        let Ok(body) = response.text().await else {
            return (None, None);
        };
        // A list, or the news that there is none yet, is kept for
        // BUILDS_TTL seconds; anything else is asked for again.
        if status == 200 || status == 404 {
            let headers = Headers::new();
            let ttl = format!("public, max-age={}", shared::BUILDS_TTL);
            if headers.set("content-type", "application/json").is_ok()
                && headers.set("cache-control", &ttl).is_ok()
            {
                if let Ok(copy) = Response::ok(body.clone()) {
                    let copy = copy.with_status(status).with_headers(headers);
                    let _ = cache.put(&url, copy).await;
                }
            }
        }
        (
            render::response(status, &body, base, shared::REPO, now),
            Some("MISS"),
        )
    }
}
