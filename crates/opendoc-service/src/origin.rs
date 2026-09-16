//! Which browser origins may talk to this service.
//!
//! Every other client of this crate is a program: the Rust client in
//! [`crate::client`], a test, the desktop shell. None of them sends an
//! `Origin` header and none of them is subject to the same-origin policy, so
//! until now the service needed no opinion about origins at all.
//!
//! A page is different in two ways, and both of them are this module's reason
//! to exist:
//!
//! 1. **It cannot reach the HTTP API without being told it may.** The token
//!    exchange, document creation and document description are cross-origin
//!    `fetch` calls from `http://127.0.0.1:<page port>` to
//!    `http://<service>`, so without `Access-Control-Allow-Origin` the browser
//!    refuses to hand the page the response. The service is not reachable from
//!    a browser at all until it answers those.
//! 2. **It will send credentials it was never asked to.** The same-origin
//!    policy does not apply to a WebSocket handshake: any page on the internet
//!    can open `ws://` to this service, and the browser will attach whatever
//!    the URL carries. The `Origin` header is the only thing that says which
//!    page did it (cross-site WebSocket hijacking). A service that accepts
//!    every origin accepts every page.
//!
//! So the policy is one allowlist used for both answers, and the default is
//! **deny**: an unconfigured service is reachable by programs and by nothing
//! in a browser. That is the safe direction to be wrong in — a missing origin
//! is a page that cannot connect, which is loud; a permissive default is a
//! page somewhere else that can, which is silent.
//!
//! A request that carries no `Origin` at all is untouched. That is not a hole:
//! a browser always sends one on a cross-origin `fetch` and on a WebSocket
//! handshake, so "no origin" means "not a page", and refusing it would only
//! break the native clients this service already has.

/// An exact-match allowlist of browser origins.
///
/// Exact match, deliberately. Wildcards and suffix matches are how origin
/// allowlists leak (`https://evil-example.com` matching `example.com`), and a
/// deployment that needs many origins can list them.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OriginPolicy {
    allowed: Vec<String>,
}

impl OriginPolicy {
    /// No browser origin is allowed. The default, and what an unconfigured
    /// service uses.
    pub fn deny_all() -> Self {
        Self {
            allowed: Vec::new(),
        }
    }

    /// The origins in `entries`, normalised. Anything that is not a plausible
    /// origin — empty, a path, a bare host with no scheme — is dropped rather
    /// than stored, so a typo cannot widen the list to something unintended.
    pub fn allowing<I, S>(entries: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut allowed: Vec<String> = entries
            .into_iter()
            .filter_map(|entry| normalize(entry.as_ref()))
            .collect();
        allowed.sort();
        allowed.dedup();
        Self { allowed }
    }

    /// A comma-separated list, as an operator supplies it in the environment.
    pub fn parse_list(value: &str) -> Self {
        Self::allowing(value.split(','))
    }

    pub fn allows(&self, origin: &str) -> bool {
        normalize(origin).is_some_and(|origin| self.allowed.contains(&origin))
    }

    pub fn is_empty(&self) -> bool {
        self.allowed.is_empty()
    }

    pub fn entries(&self) -> &[String] {
        &self.allowed
    }
}

/// Lower-cases the scheme and host, drops a trailing slash, and refuses
/// anything that is not `scheme://host[:port]`.
///
/// An origin is a *serialisation*, not a URL: `http://a.example/` and
/// `http://a.example` name the same origin, and browsers send the second form.
/// Normalising both ways means a deployment cannot fail because someone
/// pasted a URL out of the address bar.
fn normalize(value: &str) -> Option<String> {
    let value = value.trim().trim_end_matches('/');
    if value.is_empty() {
        return None;
    }
    // `null` is what a browser sends for a file:// page or a sandboxed frame.
    // It is a real origin value and must never be matchable, because it is not
    // one origin — it is every opaque one.
    if value.eq_ignore_ascii_case("null") || value == "*" {
        return None;
    }
    let (scheme, rest) = value.split_once("://")?;
    if scheme.is_empty() || rest.is_empty() || rest.contains('/') {
        return None;
    }
    if !scheme
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '+' || character == '-')
    {
        return None;
    }
    Some(format!("{}://{}", scheme.to_ascii_lowercase(), {
        // The host is case-insensitive; a port is not part of the host and has
        // no case to fold, so lower-casing the whole remainder is safe.
        rest.to_ascii_lowercase()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_policy_allows_no_browser_origin() {
        let policy = OriginPolicy::deny_all();
        assert!(policy.is_empty());
        assert!(!policy.allows("http://127.0.0.1:10185"));
        assert!(!policy.allows("https://docs.example"));
    }

    #[test]
    fn an_allowed_origin_matches_regardless_of_case_or_trailing_slash() {
        let policy = OriginPolicy::parse_list("http://127.0.0.1:10185/, HTTPS://Docs.Example");
        assert!(policy.allows("http://127.0.0.1:10185"));
        assert!(policy.allows("http://127.0.0.1:10185/"));
        assert!(policy.allows("https://docs.example"));
        assert_eq!(policy.entries().len(), 2);
    }

    #[test]
    fn matching_is_exact_so_a_neighbouring_host_or_port_is_refused() {
        let policy = OriginPolicy::parse_list("https://docs.example");
        assert!(!policy.allows("https://docs.example.evil"));
        assert!(!policy.allows("https://evil-docs.example"));
        assert!(!policy.allows("http://docs.example"));
        assert!(!policy.allows("https://docs.example:8443"));
    }

    #[test]
    fn a_wildcard_or_an_opaque_origin_is_never_stored_and_never_matches() {
        let policy = OriginPolicy::parse_list("*, null, , /just/a/path, nohost");
        assert!(policy.is_empty());
        assert!(!policy.allows("null"));
        assert!(!policy.allows("*"));
        // And a policy that *does* allow something still refuses `null`: an
        // opaque origin is every sandboxed page, not one of them.
        let policy = OriginPolicy::parse_list("https://docs.example, null");
        assert_eq!(policy.entries(), ["https://docs.example"]);
        assert!(!policy.allows("null"));
    }
}
