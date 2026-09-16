//! Fetching a URL on the user's behalf, and the policy that makes that safe.
//!
//! Which URLs may be read is a property of the runtime, not of a document,
//! which is why it lives here beside the file dialogs and not in the core (the
//! WebAssembly build has no such capability at all, and the feature is simply
//! absent there).
//!
//! # The shape of the risk
//!
//! Fetching happens with the *user's* network position, so a URL that resolves
//! inside their network reads something the document's author could not. That
//! is server-side request forgery with the user's machine as the server, and
//! `169.254.169.254` is the canonical destination.
//!
//! # Check-then-connect is not a check
//!
//! The obvious guard — resolve the host, inspect the addresses, then hand the
//! URL to an HTTP client — does not work, because the client resolves again.
//! A name with a short time-to-live that answers with a public address on the
//! first lookup and `127.0.0.1` on the second defeats it completely, and
//! checking again on every redirect hop only repeats the same gap per hop.
//!
//! So the addresses this module screens are the addresses it *connects to*:
//! [`ClientBuilder::resolve_to_addrs`](reqwest::ClientBuilder::resolve_to_addrs)
//! pins them for the host, and the client is built fresh for each hop with the
//! addresses that hop was screened on. There is no second lookup to poison.
//!
//! Proxies are off for a reason of the same shape: with `HTTP_PROXY` set, a
//! client built
//! from the environment sends the *hostname* to the proxy and the proxy does
//! the resolving, which puts the name resolution back outside this policy
//! entirely — the pinned addresses are never consulted, because the socket
//! goes to the proxy.
//!
//! `no_proxy()` on the builder is what stops that, and it is the only thing
//! that stops it. Turning off `reqwest`'s `system-proxy` feature is *not* a
//! second line of defence, however much it reads like one: `hyper-util`'s
//! proxy `Builder::from_env` reads `HTTP_PROXY`, `https_proxy` and friends
//! unconditionally, and the feature only adds the macOS and Windows
//! *system-settings* readers on top of them. The feature stays off because
//! this shell has no use for a system proxy, not because it is protecting
//! anything; `a_proxy_in_the_environment_does_not_get_the_request` is the
//! guard.
//!
//! # Proving that, rather than asserting it
//!
//! A test that only calls [`is_private_address`] proves nothing about any of
//! the above: the property is that the *socket* is opened to an address that
//! passed the screen, and nothing about a pure predicate can witness a socket.
//! So the resolver and the screen are fields of [`Network`] rather than direct
//! calls, and the tests at the bottom of this file drive the real fetch loop
//! against a real listener on the loopback with a resolver that answers one
//! way for the screen and another way for the connection. `localhost` is the
//! sharpest case: the system resolver genuinely answers `127.0.0.1` for it, so
//! any implementation that looks the name up a second time reaches the
//! listener, and the test's assertion is that the listener is never touched.

use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;

use crate::{media_type_for, FileContents};
use opendoc_app::base64_encode;
use std::path::Path;

/// Ceiling on a fetched resource, whatever the caller asks for. A picture
/// larger than this is not one a document should carry inline, and an
/// unbounded download is a way to exhaust memory from a URL.
pub(crate) const MAX_FETCH_BYTES: usize = 16 * 1024 * 1024;
/// Redirect hops followed. Each hop is screened and pinned from scratch.
const MAX_FETCH_REDIRECTS: usize = 5;
/// How long the whole fetch may take.
const FETCH_TIMEOUT_SECONDS: u64 = 30;

/// Is this address one that only this machine or this LAN can reach?
///
/// Refused rather than sandboxed: there is no legitimate document that needs a
/// picture from a link-local address.
pub(crate) fn is_private_address(address: &IpAddr) -> bool {
    match address {
        IpAddr::V4(ip) => {
            let octets = ip.octets();
            ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_documentation()
                || ip.is_unspecified()
                || ip.is_multicast()
                // 0.0.0.0/8, "this network". `is_unspecified` is only the one
                // address; on Linux the whole block routes to the local host.
                || octets[0] == 0
                // 100.64.0.0/10, carrier-grade NAT: routable, not public.
                || (octets[0] == 100 && (64..128).contains(&octets[1]))
                // 192.0.0.0/24, IETF protocol assignments — the block that
                // holds the DS-Lite and NAT64 well-known addresses.
                || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
                // 198.18.0.0/15, benchmarking: present on real networks and
                // routed to local test gear.
                || (octets[0] == 198 && (octets[1] == 18 || octets[1] == 19))
                // 240.0.0.0/4, reserved (`Ipv4Addr::is_reserved` is unstable).
                // `is_broadcast` already covers 255.255.255.255.
                || octets[0] >= 240
        }
        IpAddr::V6(ip) => {
            let segments = ip.segments();
            ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_multicast()
                // fc00::/7 unique-local and fe80::/10 link-local, spelled out
                // because the std predicates for them are still unstable.
                || (segments[0] & 0xfe00) == 0xfc00
                || (segments[0] & 0xffc0) == 0xfe80
                // fec0::/10, site-local. Deprecated by RFC 3879 twenty years
                // ago and still configured on real networks; a deprecated
                // private range is still a private range.
                || (segments[0] & 0xffc0) == 0xfec0
                // 100::/64 discard-only.
                || (segments[0] == 0x0100 && segments[1..4] == [0, 0, 0])
                // 2001::/23, IETF protocol assignments. This is the block
                // Teredo (2001::/32) lives in, and a Teredo address wraps an
                // IPv4 address that may well be a private one.
                || (segments[0] == 0x2001 && segments[1] < 0x0200)
                // 2001:db8::/32, documentation, and 3ff0::/12, which contains
                // the 3fff::/20 documentation block and is otherwise
                // unallocated — refusing the wider range costs nothing.
                || (segments[0] == 0x2001 && segments[1] == 0x0db8)
                || (segments[0] & 0xfff0) == 0x3ff0
                // 5f00::/16, segment routing.
                || segments[0] == 0x5f00
                // 2002::/16, 6to4: the next 32 bits *are* an IPv4 address, and
                // `2002:7f00:0001::` is a working spelling of 127.0.0.1.
                || (segments[0] == 0x2002
                    && is_private_address(&IpAddr::V4(embedded_v4(segments[1], segments[2]))))
                // 64:ff9b::/96 and 64:ff9b:1::/48, NAT64: same trick.
                || (segments[0] == 0x0064
                    && segments[1] == 0xff9b
                    && is_private_address(&IpAddr::V4(embedded_v4(segments[6], segments[7]))))
                // An IPv4 address wearing an IPv6 hat is still that address.
                // `to_ipv4` rather than `to_ipv4_mapped`, deliberately: it
                // catches the IPv4-*compatible* form as well, so `::127.0.0.1`
                // is refused and not merely `::ffff:127.0.0.1`.
                || ip.to_ipv4()
                    .is_some_and(|ip| is_private_address(&IpAddr::V4(ip)))
        }
    }
}

fn embedded_v4(high: u16, low: u16) -> std::net::Ipv4Addr {
    std::net::Ipv4Addr::from(((high as u32) << 16) | low as u32)
}

/// What a URL has to be before this shell will look at it at all.
///
/// Split from the address screening so both halves are pure and can be tested
/// without a network: this one answers "what host and port", the other answers
/// "may we connect to these addresses".
#[derive(Debug)]
pub(crate) enum FetchHost {
    /// The URL named an address outright, so there is nothing to resolve and
    /// nothing to pin.
    Literal(IpAddr),
    /// The URL named a host that has to be looked up.
    Domain(String),
}

pub(crate) fn fetch_host(url: &reqwest::Url) -> Result<(FetchHost, u16), String> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(format!("{} is not an http(s) URL", url.scheme()));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("URLs with credentials in them are not fetched".to_string());
    }
    let port = url.port_or_known_default().unwrap_or(80);
    let host = url
        .host_str()
        .ok_or_else(|| "URL has no host".to_string())?;
    // `host_str` brackets an IPv6 literal. Stripping the brackets before
    // parsing is what stops `http://[::1]/` being treated as a name that
    // merely fails to resolve.
    let bare = host
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(host);
    match bare.parse::<IpAddr>() {
        Ok(address) => Ok((FetchHost::Literal(address), port)),
        // Not an address as written. It may still *resolve* to one — glibc
        // accepts `0x7f000001` and `2130706433` as spellings of 127.0.0.1 —
        // which is why the screening below runs on the resolved addresses and
        // never on the name.
        Err(_) => Ok((FetchHost::Domain(host.to_string()), port)),
    }
}

/// A future that answers "what addresses does this name have".
type ResolveFuture = Pin<Box<dyn Future<Output = Result<Vec<SocketAddr>, String>> + Send>>;
/// How a name is looked up. One implementation in production, scripted ones in
/// the tests — see the module docs for why this is a field and not a call.
type Resolver = Box<dyn Fn(String, u16) -> ResolveFuture + Send + Sync>;

/// The outside world, as this module is allowed to touch it: where a name's
/// addresses come from, and which addresses are refused.
pub(crate) struct Network {
    resolve: Resolver,
    /// The screen. `is_private_address` in production; a test may widen it by
    /// one hole (the loopback) because the loopback is the only place a test
    /// can bind a listener, and the real screen refuses it first of all.
    refuses: fn(&IpAddr) -> bool,
}

impl Network {
    /// What the Tauri command runs with: the system resolver and the shipped
    /// screen.
    pub(crate) fn system() -> Self {
        Self {
            resolve: Box::new(|host, port| Box::pin(resolve_by_name(host, port))),
            refuses: is_private_address,
        }
    }
}

/// Refuse the whole request if *any* resolved address is one we will not reach.
///
/// Every address, not just the first: a name that resolves to both a public
/// and a private address must not be reachable through the public one and then
/// land on the private one. The addresses that survive are the ones the client
/// is pinned to, so this answer is the connection, not a prediction of it.
pub(crate) fn screen_addresses(
    host: &str,
    resolved: Vec<SocketAddr>,
    refuses: fn(&IpAddr) -> bool,
) -> Result<Vec<SocketAddr>, String> {
    if resolved.is_empty() {
        return Err(format!("{host} resolved to no addresses"));
    }
    if let Some(address) = resolved.iter().find(|address| refuses(&address.ip())) {
        return Err(format!(
            "{host} resolves to {}, which is on this machine or this private network",
            address.ip()
        ));
    }
    Ok(resolved)
}

/// Resolve a name off the async worker.
///
/// `ToSocketAddrs` blocks — it is a `getaddrinfo` call — and blocking inside an
/// `async fn` parks a tokio worker thread for as long as the resolver takes.
async fn resolve_by_name(host: String, port: u16) -> Result<Vec<SocketAddr>, String> {
    let name = host.clone();
    tauri::async_runtime::spawn_blocking(move || {
        std::net::ToSocketAddrs::to_socket_addrs(&(name.as_str(), port))
            .map(|addresses| addresses.collect::<Vec<_>>())
            .map_err(|err| format!("{name} could not be resolved: {err}"))
    })
    .await
    .map_err(|err| format!("{host} could not be resolved: {err}"))?
}

/// The addresses one hop will be allowed to connect to.
async fn screened_addresses(
    network: &Network,
    url: &reqwest::Url,
) -> Result<Option<(String, Vec<SocketAddr>)>, String> {
    match fetch_host(url)? {
        (FetchHost::Literal(ip), _) => {
            if (network.refuses)(&ip) {
                return Err(format!("{ip} is on this machine or this private network"));
            }
            Ok(None)
        }
        (FetchHost::Domain(host), port) => {
            let resolved = (network.resolve)(host.clone(), port).await?;
            let screened = screen_addresses(&host, resolved, network.refuses)?;
            Ok(Some((host, screened)))
        }
    }
}

fn fetch_file_name(url: &reqwest::Url) -> String {
    url.path_segments()
        .and_then(|mut segments| segments.rfind(|segment| !segment.is_empty()))
        .map(|segment| segment.to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "image".to_string())
}

/// Downloads a URL and hands back the same shape the file dialog does, so the
/// caller can feed it to `add_binary_blob` + `insert_image_block_after`
/// without a document command of its own — fetching is transport, and the
/// bytes it produces are no different from bytes off the disk.
#[tauri::command]
pub(crate) async fn fetch_url_base64(
    url: String,
    max_bytes: Option<usize>,
) -> Result<FileContents, String> {
    fetch_through(&Network::system(), &url, max_bytes).await
}

/// The fetch loop itself, over a stated [`Network`] so the property in the
/// module docs can be driven by a test rather than asserted in a comment.
async fn fetch_through(
    network: &Network,
    url: &str,
    max_bytes: Option<usize>,
) -> Result<FileContents, String> {
    let cap = max_bytes
        .unwrap_or(MAX_FETCH_BYTES)
        .clamp(1, MAX_FETCH_BYTES);
    let mut target = reqwest::Url::parse(url.trim()).map_err(|err| format!("{url}: {err}"))?;
    for _ in 0..=MAX_FETCH_REDIRECTS {
        let pinned = screened_addresses(network, &target).await?;
        let mut builder = reqwest::Client::builder()
            // Followed by hand below so every hop is screened and pinned
            // again; reqwest's own policy would resolve and connect before
            // this code saw the new host.
            .redirect(reqwest::redirect::Policy::none())
            // Without this, `HTTP_PROXY` moves name resolution into the proxy
            // and out of this policy entirely.
            .no_proxy()
            .timeout(std::time::Duration::from_secs(FETCH_TIMEOUT_SECONDS))
            .user_agent("OpenDoc");
        if let Some((host, addresses)) = &pinned {
            builder = builder.resolve_to_addrs(host, addresses);
        }
        let client = builder.build().map_err(|err| err.to_string())?;
        let mut response = client
            .get(target.clone())
            .send()
            .await
            .map_err(|err| format!("{target}: {err}"))?;
        if response.status().is_redirection() {
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| format!("{target} redirected without a location"))?;
            target = target
                .join(location)
                .map_err(|err| format!("{target} redirected to {location}: {err}"))?;
            continue;
        }
        if !response.status().is_success() {
            return Err(format!("{target} returned {}", response.status()));
        }
        // Refused before a byte is read when the server admits the size; the
        // streaming check below is what catches a server that does not.
        if let Some(length) = response.content_length() {
            if length > cap as u64 {
                return Err(format!(
                    "{target} is {length} bytes, over the {cap}-byte limit"
                ));
            }
        }
        let media_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(|value| value.split(';').next().unwrap_or(value).trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| media_type_for(Path::new(target.path())).to_string());
        let mut bytes: Vec<u8> = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|err| err.to_string())? {
            if bytes.len() + chunk.len() > cap {
                return Err(format!("{target} is larger than the {cap}-byte limit"));
            }
            bytes.extend_from_slice(&chunk);
        }
        return Ok(FileContents {
            name: fetch_file_name(&target),
            path: target.to_string(),
            media_type,
            size: bytes.len(),
            base64: base64_encode(&bytes),
        });
    }
    Err(format!(
        "{url} redirected more than {MAX_FETCH_REDIRECTS} times"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    fn ip(text: &str) -> IpAddr {
        text.parse().expect("address")
    }

    fn url(text: &str) -> reqwest::Url {
        reqwest::Url::parse(text).expect("url")
    }

    #[test]
    fn addresses_on_this_machine_or_this_network_are_private() {
        for text in [
            "127.0.0.1",
            "127.1.2.3",
            "0.0.0.0",
            // The whole "this network" block, not just the unspecified address.
            "0.1.2.3",
            "10.0.0.1",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "100.127.255.255",
            "255.255.255.255",
            "224.0.0.1",
            "192.0.2.1",
            // 192.0.0.0/24, IETF protocol assignments.
            "192.0.0.1",
            "192.0.0.171",
            // 198.18.0.0/15, benchmarking.
            "198.18.0.1",
            "198.19.255.255",
            // 240.0.0.0/4, reserved.
            "240.0.0.1",
            "255.0.0.1",
            "::1",
            "::",
            "fc00::1",
            "fd12:3456::1",
            "fe80::1",
            // fec0::/10, deprecated site-local.
            "fec0::1",
            "feff:ffff::1",
            "ff02::1",
            "::ffff:127.0.0.1",
            // IPv4-*compatible*, which `to_ipv4_mapped` does not catch.
            "::127.0.0.1",
            "::10.0.0.1",
            // 6to4 wrapping a loopback and a private address.
            "2002:7f00:0001::",
            "2002:c0a8:0101::",
            // Teredo, inside 2001::/23.
            "2001:0:4136:e378:8000:63bf:3fff:fdd2",
            // The rest of 2001::/23, which is wider than Teredo: a mutation
            // narrowing this clause to 2001::/32 survived the Teredo address
            // above, because that address has a zero second group.
            "2001:1::1",
            "2001:1ff:ffff::1",
            "2001:db8::1",
            "3fff::1",
            "100::1",
            "5f00::1",
            // NAT64 wrapping a private address.
            "64:ff9b::c0a8:101",
        ] {
            assert!(is_private_address(&ip(text)), "{text} should be private");
        }
    }

    #[test]
    fn addresses_on_the_public_internet_are_not_private() {
        for text in [
            "1.1.1.1",
            "8.8.8.8",
            "93.184.216.34",
            "172.15.255.255",
            "172.32.0.1",
            "100.63.255.255",
            "100.128.0.1",
            "192.0.1.1",
            "198.17.255.255",
            "198.20.0.1",
            "2606:4700:4700::1111",
            "2a00:1450:4001::1",
            // Immediately past 2001::/23: allocated, routed, ordinary.
            "2001:200::1",
            "2002:0808:0808::",
            "64:ff9b::0808:0808",
            "2003::1",
        ] {
            assert!(!is_private_address(&ip(text)), "{text} should be public");
        }
    }

    #[test]
    fn only_http_urls_without_credentials_are_fetched() {
        assert!(matches!(
            fetch_host(&url("https://example.invalid/a.png")),
            Ok((FetchHost::Domain(host), 443)) if host == "example.invalid"
        ));
        assert!(matches!(
            fetch_host(&url("http://example.invalid:8080/a.png")),
            Ok((FetchHost::Domain(_), 8080))
        ));
        assert!(fetch_host(&url("file:///etc/passwd"))
            .unwrap_err()
            .contains("not an http(s) URL"));
        assert!(fetch_host(&url("ftp://example.invalid/a"))
            .unwrap_err()
            .contains("not an http(s) URL"));
        assert!(fetch_host(&url("http://user:pw@example.invalid/a"))
            .unwrap_err()
            .contains("credentials"));
        assert!(fetch_host(&url("http://user@example.invalid/a"))
            .unwrap_err()
            .contains("credentials"));
    }

    /// An address in the URL is screened without ever being resolved, and the
    /// bracketed IPv6 form is recognised as an address rather than as a name
    /// that happens not to resolve.
    #[test]
    fn literal_addresses_are_recognised_as_addresses() {
        assert!(matches!(
            fetch_host(&url("http://127.0.0.1:8080/x")),
            Ok((FetchHost::Literal(_), 8080))
        ));
        let Ok((FetchHost::Literal(address), _)) = fetch_host(&url("http://[::1]/x")) else {
            panic!("bracketed IPv6 should parse as a literal address");
        };
        assert!(is_private_address(&address));

        // Hexadecimal, octal and bare-decimal spellings of an address are the
        // classic way past a guard that pattern-matches on the text of a host.
        // The `url` crate normalises them to dotted quads while parsing, so
        // they arrive here as literals and are screened as literals. That is a
        // property of a dependency rather than of this file, which is exactly
        // why it is asserted: if `url` ever stopped doing it, every one of
        // these would become a name that this module handed to a resolver.
        for spelling in [
            "http://0x7f000001/x",
            "http://2130706433/x",
            "http://0177.0.0.1/x",
            "http://127.1/x",
        ] {
            let Ok((FetchHost::Literal(address), _)) = fetch_host(&url(spelling)) else {
                panic!("{spelling} should reach the screen as an address, not as a name");
            };
            assert_eq!(address, ip("127.0.0.1"), "{spelling}");
        }
    }

    #[test]
    fn one_private_answer_refuses_the_whole_name() {
        let public: SocketAddr = "93.184.216.34:80".parse().unwrap();
        let private: SocketAddr = "127.0.0.1:80".parse().unwrap();
        assert_eq!(
            screen_addresses("host.invalid", vec![public], is_private_address).unwrap(),
            vec![public]
        );
        // Both orders: a guard that stopped at the first address would pass one
        // of these.
        assert!(
            screen_addresses("host.invalid", vec![public, private], is_private_address)
                .unwrap_err()
                .contains("127.0.0.1")
        );
        assert!(
            screen_addresses("host.invalid", vec![private, public], is_private_address)
                .unwrap_err()
                .contains("127.0.0.1")
        );
        assert!(
            screen_addresses("host.invalid", Vec::new(), is_private_address)
                .unwrap_err()
                .contains("no addresses")
        );
    }

    // ---------------------------------------------------------------------
    // Driving the real fetch loop.
    //
    // Everything below opens sockets. The point is that no assertion here can
    // be satisfied by reasoning about addresses: each one is a statement about
    // whether a particular listener was connected to.
    // ---------------------------------------------------------------------

    /// A one-connection-at-a-time HTTP/1.1 server on the loopback, and — much
    /// more to the point — a count of the requests that actually arrived.
    struct LocalServer {
        address: SocketAddr,
        hits: Arc<AtomicUsize>,
        stopping: Arc<AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl LocalServer {
        /// `replies` are served in order; the last one repeats for every
        /// further request.
        fn start(replies: Vec<String>) -> Self {
            assert!(!replies.is_empty(), "a server needs something to say");
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind the loopback");
            let address = listener.local_addr().expect("the listener's own address");
            let hits = Arc::new(AtomicUsize::new(0));
            let stopping = Arc::new(AtomicBool::new(false));
            let thread = {
                let hits = Arc::clone(&hits);
                let stopping = Arc::clone(&stopping);
                std::thread::spawn(move || {
                    let mut served = 0usize;
                    for stream in listener.incoming() {
                        if stopping.load(Ordering::SeqCst) {
                            break;
                        }
                        let Ok(mut stream) = stream else { break };
                        let mut request = Vec::new();
                        let mut byte = [0u8; 1];
                        while !request.ends_with(b"\r\n\r\n") {
                            match stream.read(&mut byte) {
                                Ok(0) | Err(_) => break,
                                Ok(_) => request.push(byte[0]),
                            }
                        }
                        // A connection that said nothing is the wake-up from
                        // `Drop`, not a request.
                        if request.is_empty() {
                            continue;
                        }
                        hits.fetch_add(1, Ordering::SeqCst);
                        let reply = replies[served.min(replies.len() - 1)].clone();
                        served += 1;
                        let _ = stream.write_all(reply.as_bytes());
                        let _ = stream.flush();
                    }
                })
            };
            Self {
                address,
                hits,
                stopping,
                thread: Some(thread),
            }
        }

        fn port(&self) -> u16 {
            self.address.port()
        }

        fn hits(&self) -> usize {
            self.hits.load(Ordering::SeqCst)
        }
    }

    impl Drop for LocalServer {
        fn drop(&mut self) {
            self.stopping.store(true, Ordering::SeqCst);
            // The thread is blocked in `accept`; one connection wakes it so it
            // can see the flag, rather than the test suite leaking a thread
            // per server.
            let _ = TcpStream::connect(self.address);
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    fn ok_reply(media_type: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {media_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    /// A body whose length the server declines to admit, so only the streaming
    /// check can stop it.
    fn undeclared_reply(body: &str) -> String {
        format!("HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n{body}")
    }

    fn redirect_reply(location: &str) -> String {
        format!("HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
    }

    /// A resolver with scripted answers and nothing else: a name not in the
    /// table resolves nowhere, which is what makes arriving at a listener
    /// evidence that the pinned addresses were used.
    fn resolving(answers: &[(&str, Vec<SocketAddr>)]) -> Resolver {
        let table: HashMap<String, Vec<SocketAddr>> = answers
            .iter()
            .map(|(host, addresses)| ((*host).to_string(), addresses.clone()))
            .collect();
        Box::new(move |host, _port| {
            let answer = table.get(&host).cloned();
            Box::pin(async move {
                answer.ok_or_else(|| format!("{host} could not be resolved: not in this test"))
            })
        })
    }

    /// The shipped screen with exactly one hole: the loopback, because the
    /// loopback is the only address a test can bind a listener to. Everything
    /// else is judged by the real rules, which is what makes the redirect test
    /// below a test of the real rules.
    fn refuses_all_but_the_loopback(address: &IpAddr) -> bool {
        !address.is_loopback() && is_private_address(address)
    }

    /// `Result::expect_err` needs `FileContents: Debug`, which it is not (and
    /// `fileaccess.rs` is not this module's to change). This says the same
    /// thing and says more when it fires.
    fn refused(outcome: Result<FileContents, String>, expectation: &str) -> String {
        match outcome {
            Ok(contents) => panic!(
                "{expectation}, but the fetch succeeded with {} bytes of {} from {}",
                contents.size, contents.media_type, contents.path
            ),
            Err(error) => error,
        }
    }

    /// The rebinding property, driven rather than argued.
    ///
    /// `localhost` is a name the *system* resolver answers `127.0.0.1` for, and
    /// the listener is on `127.0.0.1`. The scripted resolver — the one the
    /// screen sees — answers a public address instead. So an implementation
    /// that looks the name up a second time (the client's own resolver, a
    /// proxy, anything) lands on the listener, and an implementation that
    /// connects to the address it screened cannot. The assertion is the
    /// listener's connection count.
    #[tokio::test]
    async fn a_name_the_client_could_look_up_again_never_reaches_the_socket() {
        let server = LocalServer::start(vec![ok_reply("image/png", "abc")]);
        let port = server.port();
        let public: SocketAddr = format!("93.184.216.34:{port}").parse().unwrap();
        let network = Network {
            resolve: resolving(&[("localhost", vec![public])]),
            refuses: is_private_address,
        };
        // Bounded because the screened address is a public one that will not
        // answer: what is being asserted is where the fetch did *not* go, and
        // waiting out the client's own 30-second timeout to learn that is
        // pointless.
        let outcome = tokio::time::timeout(
            Duration::from_secs(2),
            fetch_through(
                &network,
                &format!("http://localhost:{port}/image.png"),
                None,
            ),
        )
        .await;
        assert_eq!(
            server.hits(),
            0,
            "the fetch connected to the listener on 127.0.0.1, which is not the address the \
             screen passed: the host is being resolved a second time, so the screened address \
             is not the connected address"
        );
        if let Ok(Ok(contents)) = &outcome {
            panic!(
                "the fetch succeeded, returning {} bytes of {} from {}",
                contents.size, contents.media_type, contents.path
            );
        }
    }

    /// The screen refuses before a socket is opened, not after.
    #[tokio::test]
    async fn a_name_that_resolves_onto_this_machine_is_refused_before_any_connection() {
        let server = LocalServer::start(vec![ok_reply("image/png", "abc")]);
        let port = server.port();
        let network = Network {
            resolve: resolving(&[("rebind.invalid", vec![server.address])]),
            refuses: is_private_address,
        };
        let error = refused(
            fetch_through(
                &network,
                &format!("http://rebind.invalid:{port}/x.png"),
                None,
            )
            .await,
            "a name resolving to 127.0.0.1 must not be fetched",
        );
        assert!(
            error.contains("127.0.0.1") && error.contains("private network"),
            "the refusal should name the address and why: {error}"
        );
        assert_eq!(
            server.hits(),
            0,
            "the request was refused and the listener was connected to anyway"
        );
    }

    /// The other half of the property: the pinned addresses are the ones that
    /// get used. `pinned.invalid` resolves nowhere on this machine, so reaching
    /// the listener at all is only possible through the addresses the screen
    /// passed to the client.
    #[tokio::test]
    async fn the_fetch_goes_to_the_screened_address_and_comes_back_with_the_bytes() {
        let server = LocalServer::start(vec![ok_reply("image/png", "abc")]);
        let port = server.port();
        let network = Network {
            resolve: resolving(&[("pinned.invalid", vec![server.address])]),
            refuses: refuses_all_but_the_loopback,
        };
        let contents = fetch_through(
            &network,
            &format!("http://pinned.invalid:{port}/picture.png"),
            None,
        )
        .await
        .expect("the screened address should be the one connected to");
        assert_eq!(server.hits(), 1);
        assert_eq!(contents.name, "picture.png");
        assert_eq!(contents.media_type, "image/png");
        assert_eq!(contents.size, 3);
        assert_eq!(contents.base64, base64_encode(b"abc"));
    }

    /// A redirect is a fresh destination and gets the whole policy again, not
    /// a second look at the first hop's verdict.
    #[tokio::test]
    async fn a_redirect_onto_a_private_address_is_refused_at_the_hop_it_appears() {
        let server = LocalServer::start(vec![redirect_reply("http://elsewhere.invalid/secret")]);
        let port = server.port();
        let network = Network {
            resolve: resolving(&[
                ("pinned.invalid", vec![server.address]),
                ("elsewhere.invalid", vec!["10.0.0.1:80".parse().unwrap()]),
            ]),
            // 10.0.0.1 is private under this screen too — only the loopback is
            // let through, and only so the first hop can exist at all.
            refuses: refuses_all_but_the_loopback,
        };
        let error = refused(
            fetch_through(
                &network,
                &format!("http://pinned.invalid:{port}/a.png"),
                None,
            )
            .await,
            "a redirect onto 10.0.0.1 must not be followed",
        );
        assert!(
            error.contains("10.0.0.1") && error.contains("private network"),
            "the second hop should be refused by the screen, naming it: {error}"
        );
        assert_eq!(server.hits(), 1, "the first hop should have been fetched");
    }

    /// A redirect cannot change the scheme into one this shell does not fetch.
    #[tokio::test]
    async fn a_redirect_off_http_is_refused() {
        let server = LocalServer::start(vec![redirect_reply("file:///etc/passwd")]);
        let port = server.port();
        let network = Network {
            resolve: resolving(&[("pinned.invalid", vec![server.address])]),
            refuses: refuses_all_but_the_loopback,
        };
        let error = refused(
            fetch_through(
                &network,
                &format!("http://pinned.invalid:{port}/a.png"),
                None,
            )
            .await,
            "a redirect to file:// must not be followed",
        );
        assert!(error.contains("not an http(s) URL"), "{error}");
    }

    /// The hop count is bounded, and bounded where it says it is.
    #[tokio::test]
    async fn a_redirect_loop_ends() {
        let server = LocalServer::start(vec![redirect_reply("/next")]);
        let port = server.port();
        let network = Network {
            resolve: resolving(&[("pinned.invalid", vec![server.address])]),
            refuses: refuses_all_but_the_loopback,
        };
        let error = refused(
            fetch_through(
                &network,
                &format!("http://pinned.invalid:{port}/a.png"),
                None,
            )
            .await,
            "a server that always redirects must not be followed forever",
        );
        // Spelled out rather than derived from `MAX_FETCH_REDIRECTS`: a test
        // that computes its expectation from the constant it guards moves
        // whenever the constant does, and a mutation raising the bound to six
        // survived exactly that way.
        assert!(error.contains("redirected more than 5 times"), "{error}");
        assert_eq!(
            server.hits(),
            6,
            "one request per allowed hop, and then the loop gives up"
        );
    }

    /// A declared length over the cap is refused without reading the body; an
    /// *undeclared* one is refused while reading it, which is the case a
    /// hostile server actually produces.
    #[tokio::test]
    async fn a_body_over_the_cap_is_refused_whether_or_not_the_server_admits_its_size() {
        let declared = LocalServer::start(vec![ok_reply("image/png", "abcdefghij")]);
        let network = Network {
            resolve: resolving(&[("pinned.invalid", vec![declared.address])]),
            refuses: refuses_all_but_the_loopback,
        };
        let error = refused(
            fetch_through(
                &network,
                &format!("http://pinned.invalid:{}/a.png", declared.port()),
                Some(4),
            )
            .await,
            "ten declared bytes should not fit in a four-byte cap",
        );
        assert!(
            error.contains("is 10 bytes, over the 4-byte limit"),
            "{error}"
        );

        let undeclared = LocalServer::start(vec![undeclared_reply("abcdefghij")]);
        let network = Network {
            resolve: resolving(&[("pinned.invalid", vec![undeclared.address])]),
            refuses: refuses_all_but_the_loopback,
        };
        let error = refused(
            fetch_through(
                &network,
                &format!("http://pinned.invalid:{}/a.png", undeclared.port()),
                Some(4),
            )
            .await,
            "ten undeclared bytes should not fit in a four-byte cap either",
        );
        assert!(error.contains("larger than the 4-byte limit"), "{error}");
    }
    /// A URL that names the address outright is refused on that address, with
    /// nothing resolved and nothing connected to.
    #[tokio::test]
    async fn a_literal_private_address_in_the_url_is_refused() {
        let server = LocalServer::start(vec![ok_reply("image/png", "abc")]);
        let network = Network {
            // Nothing resolves at all, so arriving at the listener could only
            // have come from the literal address in the URL.
            resolve: resolving(&[]),
            refuses: is_private_address,
        };
        let error = refused(
            fetch_through(
                &network,
                &format!("http://127.0.0.1:{}/a.png", server.port()),
                None,
            )
            .await,
            "a URL naming 127.0.0.1 must not be fetched",
        );
        assert!(
            error.contains("127.0.0.1 is on this machine"),
            "the refusal should name the address in the URL: {error}"
        );
        assert_eq!(server.hits(), 0);
    }

    /// `HTTP_PROXY` must not be able to take the destination out of this
    /// module's hands.
    ///
    /// A proxied request carries the *hostname* and the proxy resolves it, so
    /// the screened and pinned addresses are never consulted — the whole
    /// policy is bypassed by an environment variable. `no_proxy()` is what
    /// stops that, and nothing else does: hyper-util reads `HTTP_PROXY`
    /// whether or not reqwest's `system-proxy` feature is on.
    #[tokio::test]
    async fn a_proxy_in_the_environment_does_not_get_the_request() {
        let proxy = LocalServer::start(vec![ok_reply("text/plain", "proxied")]);
        let origin = LocalServer::start(vec![ok_reply("image/png", "abc")]);
        let network = Network {
            resolve: resolving(&[("pinned.invalid", vec![origin.address])]),
            refuses: refuses_all_but_the_loopback,
        };
        std::env::set_var("HTTP_PROXY", format!("http://127.0.0.1:{}", proxy.port()));
        let outcome = fetch_through(
            &network,
            &format!("http://pinned.invalid:{}/a.png", origin.port()),
            None,
        )
        .await;
        std::env::remove_var("HTTP_PROXY");

        assert_eq!(
            proxy.hits(),
            0,
            "the request went through the proxy named in the environment, which resolves the \
             hostname itself: the screened addresses were never used"
        );
        assert_eq!(origin.hits(), 1, "the pinned address should have been used");
        let contents = outcome.unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(contents.base64, base64_encode(b"abc"));
    }
}
