use anyhow::{Context, Result, bail};
use reqwest::blocking::Client;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue, RETRY_AFTER, USER_AGENT};
use reqwest::{Method, StatusCode};
use serde_json::Value;
use std::cell::Cell;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// A completed HTTP rejection proves Discord did not accept the message.
/// Transport errors, timeouts and 5xx responses deliberately do not use this.
#[derive(Debug)]
pub struct Rejected(pub u16);

impl std::fmt::Display for Rejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Discord rejected the request with HTTP {}", self.0)
    }
}

impl std::error::Error for Rejected {}

pub struct Api {
    client: Client,
    base: &'static str,
    github: bool,
    not_before: Cell<SystemTime>,
}

impl Api {
    pub fn new(github: bool, token: Option<&str>) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static("sekka-discord/0.1 (bitemyapp/sekka)"),
        );
        if let Some(token) = token {
            let scheme = if github { "Bearer" } else { "Bot" };
            let mut value = HeaderValue::from_str(&format!("{scheme} {token}"))
                .map_err(|_| anyhow::anyhow!("Token contains invalid HTTP header characters"))?;
            value.set_sensitive(true);
            headers.insert(AUTHORIZATION, value);
        }
        if github {
            headers.insert(
                "accept",
                HeaderValue::from_static("application/vnd.github+json"),
            );
            headers.insert(
                "x-github-api-version",
                HeaderValue::from_static("2022-11-28"),
            );
        }
        Ok(Self {
            client: Client::builder()
                .default_headers(headers)
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(35))
                .build()
                .context("Creating HTTPS client")?,
            base: if github {
                "https://api.github.com"
            } else {
                "https://discord.com/api/v10"
            },
            github,
            not_before: Cell::new(UNIX_EPOCH),
        })
    }

    pub fn get(&self, path: &str) -> Result<Value> {
        self.request(Method::GET, path, None)
    }

    pub fn request(&self, method: Method, path: &str, body: Option<&Value>) -> Result<Value> {
        loop {
            if let Ok(delay) = self.not_before.get().duration_since(SystemTime::now()) {
                std::thread::sleep(delay);
            }
            let mut request = self
                .client
                .request(method.clone(), format!("{}{path}", self.base));
            if let Some(body) = body {
                request = request.json(body);
            }
            let response = request
                .send()
                .context("REST request failed (delivery may be uncertain)")?;
            let status = response.status();
            let headers = response.headers();
            if self.github {
                if number_header(headers, "x-ratelimit-remaining") == Some(0)
                    && let Some(reset) = reset_time(headers)
                {
                    self.not_before.set(reset);
                }
            }
            let retry_header = headers
                .get(RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(retry_after);
            let reset = reset_time(headers);
            let remaining = number_header(headers, "x-ratelimit-remaining");
            let value: Value = response.json().unwrap_or(Value::Null);
            let secondary_limit = self.github
                && status == StatusCode::FORBIDDEN
                && value["message"]
                    .as_str()
                    .is_some_and(|s| s.to_ascii_lowercase().contains("rate limit"));
            let limited = status == StatusCode::TOO_MANY_REQUESTS
                || (self.github
                    && status == StatusCode::FORBIDDEN
                    && (retry_header.is_some() || remaining == Some(0) || secondary_limit));
            if limited {
                let json_delay = value["retry_after"]
                    .as_f64()
                    .and_then(|n| Duration::try_from_secs_f64(n).ok());
                let now = SystemTime::now();
                let mut delay = retry_header
                    .or(json_delay)
                    .unwrap_or(Duration::from_secs(60));
                if self.github
                    && let Some(reset_delay) = reset.and_then(|t| t.duration_since(now).ok())
                {
                    delay = delay.max(reset_delay);
                }
                delay = delay.max(Duration::from_secs(1));
                eprintln!(
                    "{} rate limit: waiting {} seconds",
                    if self.github { "GitHub" } else { "Discord" },
                    delay.as_secs()
                );
                self.not_before.set(
                    now.checked_add(delay)
                        .context("Rate-limit deadline overflow")?,
                );
                continue;
            }
            if !status.is_success() {
                if !self.github && status.is_client_error() && status != StatusCode::REQUEST_TIMEOUT
                {
                    return Err(Rejected(status.as_u16()).into());
                }
                bail!(
                    "{} REST request returned HTTP {}",
                    if self.github { "GitHub" } else { "Discord" },
                    status.as_u16()
                );
            }
            if value.is_null() && status != StatusCode::NO_CONTENT {
                bail!("REST endpoint returned invalid or empty JSON");
            }
            return Ok(value);
        }
    }
}

fn number_header(headers: &HeaderMap, name: &str) -> Option<u64> {
    headers.get(name)?.to_str().ok()?.parse().ok()
}

fn reset_time(headers: &HeaderMap) -> Option<SystemTime> {
    UNIX_EPOCH.checked_add(Duration::from_secs(
        number_header(headers, "x-ratelimit-reset")?.saturating_add(1),
    ))
}

fn retry_after(value: &str) -> Option<Duration> {
    if let Ok(seconds) = value.parse::<f64>() {
        return Duration::try_from_secs_f64(seconds).ok();
    }
    Some(
        httpdate::parse_http_date(value)
            .ok()?
            .duration_since(SystemTime::now())
            .unwrap_or_default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_seconds_are_validated() {
        assert_eq!(retry_after("1.25"), Some(Duration::from_millis(1250)));
        assert_eq!(retry_after("NaN"), None);
        assert_eq!(retry_after("-1"), None);
        assert_eq!(
            retry_after("Wed, 21 Oct 2015 07:28:00 GMT"),
            Some(Duration::ZERO)
        );
    }
}
