//! The HTTP layer behind a trait, so everything above it is tested without a
//! network. The real one is `ureq` with rustls, timeouts and a fixed
//! User-Agent; no cookies, no other identifying headers.

use crate::Error;
use std::io::Read;
use std::time::Duration;

/// An HTTP response: status and body stream.
pub struct Response {
    pub status: u16,
    pub content_length: Option<u64>,
    pub body: Box<dyn Read + Send>,
}

impl std::fmt::Debug for Response {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Response").field("status", &self.status).field("content_length", &self.content_length).finish()
    }
}

pub trait Http: Send + Sync {
    /// GET `url` (following redirects); with `range_from` the request asks
    /// for the bytes from that offset (`Range: bytes=N-`). Non-2xx statuses
    /// are returned, not turned into errors.
    fn get(&self, url: &str, range_from: Option<u64>) -> Result<Response, Error>;
}

/// The User-Agent: the product and version, nothing else.
pub fn user_agent(version: &str) -> String {
    format!("OpenDecks-Session/{version}")
}

/// GETs a small text resource (at most `limit` bytes).
pub fn get_text(http: &dyn Http, url: &str, limit: u64) -> Result<String, Error> {
    let r = http.get(url, None)?;
    if r.status != 200 {
        return Err(Error::Status(r.status, url.to_string()));
    }
    let mut s = String::new();
    r.body.take(limit).read_to_string(&mut s).map_err(|e| Error::Network(e.to_string()))?;
    Ok(s)
}

/// `ureq` 3 with rustls.
pub struct UreqHttp {
    agent: ureq::Agent,
}

impl UreqHttp {
    pub fn new(version: &str) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .user_agent(user_agent(version))
            .https_only(true)
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_recv_response(Some(Duration::from_secs(30)))
            // Large downloads: no overall limit, but a stalled body ends.
            .timeout_recv_body(Some(Duration::from_secs(3600)))
            .http_status_as_error(false)
            .max_redirects(10)
            .build()
            .into();
        Self { agent }
    }
}

impl Http for UreqHttp {
    fn get(&self, url: &str, range_from: Option<u64>) -> Result<Response, Error> {
        let mut req = self.agent.get(url);
        if url.starts_with("https://api.github.com/") {
            req = req.header("Accept", "application/vnd.github+json").header("X-GitHub-Api-Version", "2022-11-28");
        }
        if let Some(n) = range_from.filter(|n| *n > 0) {
            req = req.header("Range", format!("bytes={n}-"));
        }
        let resp = req.call().map_err(|e| Error::Network(e.to_string()))?;
        let status = resp.status().as_u16();
        let content_length = resp.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok());
        let body = resp.into_body().into_reader();
        Ok(Response { status, content_length, body: Box::new(body) })
    }
}

/// An in-memory HTTP server for tests: URL → body. Honours `Range`,
/// records requests and can cut a body short to simulate a dropped
/// connection.
#[cfg(test)]
pub(crate) mod mock {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    pub struct MockHttp {
        pub files: HashMap<String, Vec<u8>>,
        /// URL → bytes after which the next response ends (once).
        pub cut: Mutex<HashMap<String, usize>>,
        /// URLs that ignore Range and always answer 200.
        pub no_range: Vec<String>,
        pub requests: Mutex<Vec<(String, Option<u64>)>>,
    }

    impl MockHttp {
        pub fn with(mut self, url: &str, body: impl Into<Vec<u8>>) -> Self {
            self.files.insert(url.to_string(), body.into());
            self
        }
    }

    impl Http for MockHttp {
        fn get(&self, url: &str, range_from: Option<u64>) -> Result<Response, Error> {
            self.requests.lock().unwrap().push((url.to_string(), range_from));
            let Some(data) = self.files.get(url) else {
                return Ok(Response { status: 404, content_length: Some(0), body: Box::new(std::io::empty()) });
            };
            let (status, mut part) = match range_from.filter(|n| *n > 0 && !self.no_range.iter().any(|u| u == url)) {
                Some(n) if n as usize >= data.len() => return Ok(Response { status: 416, content_length: Some(0), body: Box::new(std::io::empty()) }),
                Some(n) => (206, data[n as usize..].to_vec()),
                None => (200, data.clone()),
            };
            let len = part.len() as u64;
            if let Some(c) = self.cut.lock().unwrap().remove(url) {
                part.truncate(c);
            }
            Ok(Response { status, content_length: Some(len), body: Box::new(std::io::Cursor::new(part)) })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_agent_names_only_product_and_version() {
        assert_eq!(user_agent("1.2.3"), "OpenDecks-Session/1.2.3");
    }

    #[test]
    fn text_needs_200() {
        let m = mock::MockHttp::default().with("https://x/a", "hello");
        assert_eq!(get_text(&m, "https://x/a", 100).unwrap(), "hello");
        assert_eq!(get_text(&m, "https://x/a", 3).unwrap(), "hel");
        assert!(matches!(get_text(&m, "https://x/missing", 100), Err(Error::Status(404, _))));
    }
}
