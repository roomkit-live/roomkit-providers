//! How a client's HTTP client is handed the proxies of the environment, and
//! how a client leaves them: a provider a test points at a local fake, and
//! one a replay drives, send there directly.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::Proxies;

/// Where a client sends its requests: through the proxies the environment
/// named when it was built, or directly once it is pointed at a local fake
/// of its endpoint. The HTTP client asks it for each request, so what a
/// client is pointed at holds for its clones too.
pub(crate) struct Route {
    proxies: Proxies,
    /// Whether the client was pointed at a local fake: no request goes
    /// through a proxy from then on.
    direct: AtomicBool,
}

impl Route {
    /// The route of a client that sends through `proxies`.
    pub(crate) fn through(proxies: Proxies) -> Self {
        Self {
            proxies,
            direct: AtomicBool::new(false),
        }
    }

    /// The proxies the environment named, which the client was built with:
    /// they stay its secrets once it sends directly.
    pub(crate) fn proxies(&self) -> &Proxies {
        &self.proxies
    }

    /// Sends directly from now on, through none of the proxies.
    #[cfg(any(test, feature = "replay", feature = "harness"))]
    pub(crate) fn go_direct(&self) {
        self.direct.store(true, Ordering::Relaxed);
    }

    /// The rules the HTTP client is given: one for each proxy, which sends
    /// through it the requests the proxy's place among the rules gives it.
    /// A proxy's credentials are given beside its address, as a user and a
    /// password the HTTP client writes in a header: never inside the
    /// address, which the HTTP client reads again.
    pub(crate) fn rules(self: &Arc<Self>) -> Vec<reqwest::Proxy> {
        let rules = self.proxies.named().map(|(place, proxy)| {
            let route = Arc::clone(self);
            let rule = reqwest::Proxy::custom(move |url| route.dialed(place, url));
            match &proxy.credentials {
                Some(given) => rule.basic_auth(&given.user, &given.password),
                None => rule,
            }
        });
        rules.collect()
    }

    /// The address of the proxy a request for `url` is sent through, when
    /// it is the proxy of the rule at `place`.
    fn dialed(&self, place: usize, url: &reqwest::Url) -> Option<reqwest::Url> {
        if self.direct.load(Ordering::Relaxed) {
            return None;
        }
        let (named_at, proxy) = self.proxies.for_url(url)?;
        (named_at == place).then(|| proxy.address.clone())
    }
}
