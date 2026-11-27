//! The edge a provider with no pinned region asks, as polargrid-sdk picks
//! it (`_discover_best_region`): the autorouter is asked for the nearest
//! edge that already serves the model, and its pick is kept for as long as
//! it says, a day when it says nothing, in a cache the whole process
//! shares. An autorouter that does not answer a pick, whatever the reason,
//! sends the request to the SDK's default edge, whose pick is not kept.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use reqwest::header::{ACCEPT, HeaderMap, HeaderValue};
use roomkit_providers_client::chat::truthy;
use roomkit_providers_client::failure::past_mark;
use roomkit_providers_client::{Call, Client};
use serde_json::Value;

use crate::catalogue;

/// The autorouter.
pub(crate) const AUTOROUTER: &str = "https://autorouter.polargrid.ai";

/// How long a pick is kept when the autorouter says nothing of it, in
/// seconds.
const KEPT_A_DAY: f64 = 86_400.0;

/// An edge a provider sends its requests to: its URL, and the id and the
/// name it is known by.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Pick {
    pub(crate) endpoint: String,
    pub(crate) region_id: Option<String>,
    pub(crate) region_name: Option<String>,
}

/// The picks kept, by the query they answer, each until when.
#[derive(Debug, Default)]
pub(crate) struct Picks(Mutex<HashMap<String, (Pick, f64)>>);

/// The picks the whole process shares, as the SDK's are.
static SHARED: LazyLock<Arc<Picks>> = LazyLock::new(Arc::default);

impl Picks {
    /// The cache every provider of the process shares.
    pub(crate) fn shared() -> Arc<Self> {
        Arc::clone(&SHARED)
    }

    fn kept(&self, query: &str, now: f64) -> Option<Pick> {
        let mut picks = self.0.lock().ok()?;
        match picks.get(query) {
            Some((pick, until)) if *until > now => Some(pick.clone()),
            Some(_) => {
                picks.remove(query);
                None
            }
            None => None,
        }
    }

    fn keep(&self, query: &str, pick: &Pick, until: f64) {
        if let Ok(mut picks) = self.0.lock() {
            picks.insert(query.to_owned(), (pick.clone(), until));
        }
    }
}

/// The query the autorouter is asked `model` with, the cache key too.
fn query(model: &str) -> String {
    let model = model.trim();
    if model.is_empty() {
        return String::new();
    }
    let mut written = reqwest::Url::parse("http://query.invalid/").ok();
    let Some(written) = written.as_mut() else {
        return String::new();
    };
    written.query_pairs_mut().append_pair("model", model);
    written.query().unwrap_or_default().to_owned()
}

/// The edge to send the requests of `model` to: the one kept for it, else
/// the autorouter's pick, kept as long as it says, else the SDK's default
/// edge. `router` is the autorouter's URL, `client` the client that asks
/// it.
pub(crate) async fn pick(
    client: &Client,
    router: &reqwest::Url,
    model: &str,
    picks: &Picks,
) -> Pick {
    let query = query(model);
    let now = client.now();
    if let Some(kept) = picks.kept(&query, now) {
        return kept;
    }
    if let Some((pick, ttl)) = asked(client, router, &query).await {
        picks.keep(&query, &pick, now + ttl);
        return pick;
    }
    let (endpoint, region_id, region_name) = catalogue::fallback();
    Pick {
        endpoint,
        region_id: Some(region_id),
        region_name,
    }
}

/// What the autorouter picks, and for how long, when it answers a pick.
async fn asked(client: &Client, router: &reqwest::Url, query: &str) -> Option<(Pick, f64)> {
    let mut url = router.clone();
    url.set_path("/v1/route");
    url.set_query(Some(query).filter(|query| !query.is_empty()));
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    let call = Call {
        url: &url,
        headers: &headers,
        body: None,
        token: None,
    };
    let (body, _) = client.send(call).await.ok()?;
    let answer: Value = serde_json::from_slice(past_mark(&body)).ok()?;
    let answer = answer.as_object()?;
    let region = answer.get("region").filter(|region| truthy(region))?;
    let endpoint = answer.get("endpoint").filter(|endpoint| truthy(endpoint))?;
    let text = |value: &Value| {
        value
            .as_str()
            .map_or_else(|| value.to_string(), str::to_owned)
    };
    // A name the answer leaves out is the region's; one it gives as null is
    // none, as Python's `pick.get('name', region)` reads it.
    let name = answer.get("name").map_or(Some(region), |name| {
        Some(name).filter(|name| !name.is_null())
    });
    // Python reads a `true` as the number 1, its booleans being numbers.
    let ttl = match answer.get("ttl") {
        Some(Value::Bool(true)) => Some(1.0),
        Some(ttl) => ttl.as_f64(),
        None => None,
    };
    let ttl = ttl.filter(|ttl| *ttl > 0.0);
    let pick = Pick {
        endpoint: text(endpoint),
        region_id: Some(text(region)),
        region_name: name.map(text),
    };
    Some((pick, ttl.unwrap_or(KEPT_A_DAY)))
}

// Python: polargrid-sdk's _discover_best_region, whose cache a replay cannot
// see: each case is one call.
#[cfg(test)]
mod tests {
    use roomkit_providers_client::harness::fake::{Fake, Reply};

    use super::*;
    use crate::{PolarGrid, PolarGridConfig};

    const TORONTO: &str = "https://api.yto-01.edge.polargrid.ai";

    /// The picks of `asked` calls of a provider whose autorouter answers
    /// `answers`, the cache they share, how many requests it got, and when
    /// the first call started.
    async fn picked(answers: Vec<Reply>, asked: usize) -> (Vec<Pick>, Picks, usize, f64) {
        let fake = Fake::serving(answers).await;
        let mut provider = PolarGrid::new(PolarGridConfig::new("placeholder-not-a-key")).unwrap();
        provider.point_at(fake.address);
        let (client, router) = (&provider.router_client, &provider.router);
        let picks = Picks::default();
        let started = client.now();
        let mut got = Vec::new();
        for _ in 0..asked {
            got.push(pick(client, router, "m", &picks).await);
        }
        (got, picks, fake.seen().len(), started)
    }

    fn answered(body: &str) -> Vec<Reply> {
        vec![Reply::json(200, body.to_owned())]
    }

    fn fallback() -> Pick {
        Pick {
            endpoint: TORONTO.into(),
            region_id: Some("yto-01".into()),
            region_name: Some("Toronto".into()),
        }
    }

    #[tokio::test]
    async fn a_pick_is_asked_once_then_kept() {
        let pick = r#"{"region": "yul-02", "endpoint": "https://api.yul-02.edge.polargrid.ai", "name": "Montreal 02"}"#;
        let (got, _, asked, _) = picked(answered(pick), 2).await;
        let montreal = Pick {
            endpoint: "https://api.yul-02.edge.polargrid.ai".into(),
            region_id: Some("yul-02".into()),
            region_name: Some("Montreal 02".into()),
        };
        assert_eq!(got, [montreal.clone(), montreal]);
        assert_eq!(asked, 1);
    }

    #[tokio::test]
    async fn a_pick_without_a_name_is_named_by_its_region_one_named_null_is_not() {
        let unnamed = r#"{"region": "yul-02", "endpoint": "https://e"}"#;
        let (got, ..) = picked(answered(unnamed), 1).await;
        assert_eq!(got[0].region_name.as_deref(), Some("yul-02"));
        let null = r#"{"region": "yul-02", "endpoint": "https://e", "name": null}"#;
        let (got, ..) = picked(answered(null), 1).await;
        assert_eq!(got[0].region_name, None);
    }

    #[tokio::test]
    async fn a_pick_lacking_its_region_or_its_endpoint_falls_back_and_is_not_kept() {
        let answers = vec![
            Reply::json(200, r#"{"region": "", "endpoint": "https://e"}"#),
            Reply::json(200, r#"{"region": "yul-02"}"#),
            Reply::json(200, r#"["yul-02"]"#),
        ];
        let (got, _, asked, _) = picked(answers, 3).await;
        assert_eq!(got, [fallback(), fallback(), fallback()]);
        assert_eq!(asked, 3);
    }

    #[tokio::test]
    async fn a_pick_is_kept_as_long_as_it_says_else_a_day() {
        let kept = [
            ("60", 60.0),
            ("0.5", 0.5),
            ("true", 1.0),
            ("0", KEPT_A_DAY),
            ("-5", KEPT_A_DAY),
            ("false", KEPT_A_DAY),
            (r#""60""#, KEPT_A_DAY),
            ("null", KEPT_A_DAY),
        ];
        for (ttl, seconds) in kept {
            let pick = format!(r#"{{"region": "r", "endpoint": "https://e", "ttl": {ttl}}}"#);
            let (_, picks, _, started) = picked(answered(&pick), 1).await;
            let query = query("m");
            assert!(
                picks.kept(&query, started + seconds - 0.1).is_some(),
                "{ttl}"
            );
            assert!(
                picks.kept(&query, started + seconds + 0.9).is_none(),
                "{ttl}"
            );
        }
    }

    #[test]
    fn the_query_names_the_model_as_python_s_urlencode_writes_it() {
        assert_eq!(query("qwen-3.8-27b"), "model=qwen-3.8-27b");
        assert_eq!(query(" qwen 3 "), "model=qwen+3");
        assert_eq!(query("  "), "");
    }

    #[test]
    fn a_pick_is_kept_until_its_time() {
        let picks = Picks::default();
        let pick = Pick {
            endpoint: "https://api.yto-01.edge.polargrid.ai".into(),
            region_id: Some("yto-01".into()),
            region_name: Some("Toronto".into()),
        };
        picks.keep("model=m", &pick, 100.0);
        assert_eq!(picks.kept("model=m", 99.0), Some(pick));
        assert_eq!(picks.kept("model=m", 100.0), None);
        assert_eq!(picks.kept("model=m", 0.0), None);
        assert_eq!(picks.kept("model=other", 0.0), None);
    }
}
