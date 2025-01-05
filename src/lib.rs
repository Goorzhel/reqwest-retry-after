//! # reqwest-retry-after
//!
//! `reqwest-retry-after` is a library that adds support for the `Retry-After` header
//! in [`reqwest`], using [`reqwest_middleware`].
//!
//! ## Usage
//!
//! Pass [`RetryAfterMiddleware`] to the [`ClientWithMiddleware`] builder.
//!
//! ```
//! use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
//! use reqwest_retry_after::RetryAfterMiddleware;
//!
//! let client = ClientBuilder::new(reqwest::Client::new())
//!     .with(RetryAfterMiddleware::new())
//!     .build();
//! ```
#![warn(missing_docs)]

#[cfg(test)]
mod test;

use std::{
    collections::HashMap,
    time::{Duration, SystemTime},
};

use http::{header::RETRY_AFTER, Extensions};
use reqwest::Url;
use reqwest_middleware::{
    reqwest::{Request, Response},
    Middleware, Next, Result,
};
use time::{format_description::well_known::Rfc2822, OffsetDateTime};
use tokio::sync::RwLock;

/// The `RetryAfterMiddleware` is a [`Middleware`] that adds support for the `Retry-After`
/// header in [`reqwest`].
pub struct RetryAfterMiddleware {
    retry_after: RwLock<HashMap<Url, SystemTime>>,
}

impl RetryAfterMiddleware {
    /// Creates a new `RetryAfterMiddleware`.
    pub fn new() -> Self {
        Self {
            retry_after: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for RetryAfterMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

fn parse_retry_value(val: &str) -> Option<SystemTime> {
    if let Ok(secs) = val.parse::<u64>() {
        return Some(SystemTime::now() + Duration::from_secs(secs));
    }
    if let Ok(date) = OffsetDateTime::parse(val, &Rfc2822) {
        return Some(date.into());
    }
    None
}

#[async_trait::async_trait]
impl Middleware for RetryAfterMiddleware {
    async fn handle(
        &self,
        req: Request,
        extensions: &mut Extensions,
        next: Next<'_>,
    ) -> Result<Response> {
        let url = req.url().clone();

        if let Some(timestamp) = self.retry_after.read().await.get(&url) {
            let now = SystemTime::now();

            if let Ok(duration) = timestamp.duration_since(now) {
                tokio::time::sleep(duration).await;
            }
        }

        let res = next.run(req, extensions).await;

        if let Ok(res) = &res {
            match res.headers().get(RETRY_AFTER) {
                Some(retry_after) => {
                    if let Ok(val) = retry_after.to_str() {
                        if let Some(timestamp) = parse_retry_value(val) {
                            self.retry_after
                                .write()
                                .await
                                .insert(url.clone(), timestamp);
                        }
                    }
                }
                _ => {
                    self.retry_after.write().await.remove(&url);
                }
            }
        }
        res
    }
}
