#![cfg_attr(doc, doc = include_str!("../README.md"))]

use core::future::Future;
use core::pin::Pin;
use core::time::Duration;
use std::time::SystemTime;

/// Start a client builder with the selected TLS backend and same-origin redirects.
/// rustls takes precedence when both TLS features are enabled.
pub fn client_builder() -> reqwest::ClientBuilder {
    let builder =
        reqwest::Client::builder().redirect(reqwest::redirect::Policy::custom(|attempt| {
            if !attempt
                .previous()
                .last()
                .is_some_and(|url| url.origin() == attempt.url().origin())
            {
                attempt.stop()
            } else if attempt.previous().len() > 10 {
                attempt.error("too many redirects")
            } else {
                attempt.follow()
            }
        }));
    #[cfg(feature = "rustls")]
    let builder = builder.tls_backend_rustls();
    #[cfg(all(feature = "native-tls", not(feature = "rustls")))]
    let builder = builder.tls_backend_native();
    builder
}

/// A bounded retry policy for explicit HTTP 429 responses.
///
/// Transport failures are never retried. The default replays only GET, HEAD and
/// OPTIONS requests, and only when their bodies can be cloned by reqwest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    max_retries: u8,
    initial_delay: Duration,
    max_delay: Duration,
    non_idempotent: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self::new(2, Duration::from_millis(200), Duration::from_secs(30))
    }
}

impl RetryPolicy {
    /// Set the number of additional attempts and the backoff bounds.
    /// A Retry-After value above `max_delay` returns the 429 without waiting.
    #[must_use]
    pub const fn new(max_retries: u8, initial_delay: Duration, max_delay: Duration) -> Self {
        Self {
            max_retries,
            initial_delay,
            max_delay,
            non_idempotent: false,
        }
    }

    /// Send every request exactly once.
    #[must_use]
    pub const fn none() -> Self {
        Self::new(0, Duration::ZERO, Duration::ZERO)
    }

    /// Permit replay of mutation requests with cloneable bodies after a 429.
    ///
    /// Enable only when the endpoint rejects rate-limited requests before any
    /// side effect. This is not a general idempotency guarantee for POST or PATCH.
    #[must_use]
    pub const fn retry_non_idempotent(mut self, enabled: bool) -> Self {
        self.non_idempotent = enabled;
        self
    }

    const fn permits(self, method: &reqwest::Method) -> bool {
        self.max_retries != 0
            && (self.non_idempotent
                || matches!(
                    *method,
                    reqwest::Method::GET | reqwest::Method::HEAD | reqwest::Method::OPTIONS
                ))
    }

    fn delay(self, response: &reqwest::Response, attempt: u8) -> Option<Duration> {
        let backoff = self
            .initial_delay
            .saturating_mul(2_u32.saturating_pow(u32::from(attempt)))
            .min(self.max_delay);
        let delay = match response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
        {
            Some(value) if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) => {
                Duration::from_secs(value.parse().ok()?)
            }
            Some(value) => httpdate::parse_http_date(value)
                .ok()
                .map_or(backoff, |date| {
                    date.duration_since(SystemTime::now())
                        .unwrap_or(Duration::ZERO)
                }),
            None => backoff,
        };
        (delay <= self.max_delay).then_some(delay)
    }
}

/// Execute a request with the supplied rate-limit retry policy.
///
/// The final response is unchanged, including its headers and body. Streaming
/// bodies that reqwest cannot clone are sent once. Dropping this future cancels
/// an in-flight request or a pending backoff.
///
/// The retry state is boxed so it does not enlarge callers' async state machines.
///
/// # Errors
/// Returns reqwest's transport error immediately, without replaying the request.
pub fn execute(
    client: &reqwest::Client,
    mut request: reqwest::Request,
    policy: RetryPolicy,
) -> Pin<Box<impl Future<Output = Result<reqwest::Response, reqwest::Error>> + Send + '_>> {
    Box::pin(async move {
        let replay = policy.permits(request.method());
        let mut attempt = 0;
        loop {
            // Sending consumes the current request. Retain only the next attempt.
            let next = if replay && attempt < policy.max_retries {
                request.try_clone()
            } else {
                None
            };
            let response = client.execute(request).await?;
            if response.status() != reqwest::StatusCode::TOO_MANY_REQUESTS {
                return Ok(response);
            }
            let Some(next) = next else {
                return Ok(response);
            };
            let Some(delay) = policy.delay(&response, attempt) else {
                return Ok(response);
            };
            drop(response);
            tokio::time::sleep(delay).await;
            attempt = attempt.saturating_add(1);
            request = next;
        }
    })
}
