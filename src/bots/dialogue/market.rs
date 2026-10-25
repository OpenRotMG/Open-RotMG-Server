use super::*;
use crate::bots::pursuits::market::{Message, goods_text, parse_quote};

pub(super) struct Wording {
    facts: String,
    created: Instant,
    result: Arc<Mutex<Option<std::result::Result<String, Failure>>>>,
}

pub(super) const PROMPT: &str = "Generate your own terse 2012 Realm player trade speech. Use reply_to_player once with the sole allowed intent. Write a TEMPLATE: never substitute facts into tokens. For market_advertise use SELL {give} LINK {want} or BUY {want} LINK {give}, optionally followed by ' @{name}'. SELL is wts, selling, sell, s>, or trading. BUY is wtb, buying, or buy. LINK is for or =. Caps and extra whitespace are allowed. For market_refuse the grammar is 'nty need {want}' or 'nah need {want}' or 'no thx need {want}'. Vary wording and capitalization for the supplied speaker/style, keeping their preferred voice. Tokens must occur exactly once. No other words, numbers, punctuation, goods, promises, acceptance or invented terms. The program inserts inventory-validated facts AFTER generation. WINDOW offers are the only trade acceptance authority.";

fn render(
    template: &str,
    intent: Intent,
    give: &str,
    want: &str,
    name: &str,
) -> std::result::Result<String, Failure> {
    let words = template.split_whitespace().collect::<Vec<_>>();
    let lower = words
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let valid = match intent {
        Intent::MarketAdvertise => {
            (words.len() == 4 || (words.len() == 5 && lower[4] == "@{name}"))
                && matches!(lower[2].as_str(), "for" | "=")
                && ((matches!(
                    lower[0].as_str(),
                    "wts" | "selling" | "sell" | "s>" | "trading"
                ) && lower[1] == "{give}"
                    && lower[3] == "{want}")
                    || (matches!(lower[0].as_str(), "wtb" | "buying" | "buy")
                        && lower[1] == "{want}"
                        && lower[3] == "{give}"))
        }
        Intent::MarketRefuse => {
            matches!(lower.as_slice(), [a,b,c] if matches!(a.as_str(), "nty" | "nah") && b == "need" && c == "{want}")
                || matches!(lower.as_slice(), [a,b,c,d] if a == "no" && b == "thx" && c == "need" && d == "{want}")
        }
        _ => false,
    };
    if !valid {
        return Err(Failure("invalid_market_wording"));
    }
    let mut result = Vec::new();
    for (word, token) in words.iter().zip(lower) {
        result.push(match token.as_str() {
            "{give}" => give.to_owned(),
            "{want}" => want.to_owned(),
            "@{name}" => format!("@{name}"),
            _ => (*word).to_owned(),
        });
    }
    let text = result.join(" ");
    crate::world::chat::validate(&text).map_err(|_| Failure("invalid_market_wording"))?;
    if text.encode_utf16().count() > 128 {
        return Err(Failure("invalid_market_wording"));
    }
    Ok(text)
}

impl Service {
    pub(in crate::bots) fn market_wording(
        self: &Arc<Self>,
        name: &str,
        message: &Message,
        c: &Content,
        personality: Value,
    ) -> Result<Option<String>> {
        let (intent, give, want) = if let Some(q) = parse_quote(&message.text, c) {
            (
                Intent::MarketAdvertise,
                goods_text(&q[0], c)?,
                goods_text(&q[1], c)?,
            )
        } else {
            (
                Intent::MarketRefuse,
                String::new(),
                message
                    .text
                    .strip_prefix("nty need ")
                    .context("invalid market speech intent")?
                    .to_owned(),
            )
        };
        let key = format!("{name}:{intent:?}");
        let mut cache = self.wordings.lock().expect("market speech cache poisoned");
        if let Some(entry) = cache.get(&key) {
            let result = entry.result.lock().expect("market speech result poisoned");
            if entry.facts == message.text {
                match result.as_ref() {
                    Some(Ok(text)) => return Ok(Some(text.clone())),
                    Some(Err(error)) if entry.created.elapsed() < Duration::from_secs(60) => {
                        return Err((*error).into());
                    }
                    Some(Err(_)) => {}
                    None => return Ok(None),
                }
            }
            // Complete and count a stale request, but never publish it for changed stock/terms.
            if result.is_none() {
                return Ok(None);
            }
        }
        if !self.automatic_available() {
            return Ok(None);
        }
        ensure!(
            cache.contains_key(&key) || cache.len() < 256,
            "market speech speaker cache full"
        );
        let result = Arc::new(Mutex::new(None));
        cache.insert(
            key,
            Wording {
                facts: message.text.clone(),
                created: Instant::now(),
                result: result.clone(),
            },
        );
        let service = self.clone();
        let name = name.to_owned();
        let payload = json!({"allowed_intents":[intent],"automatic":true,"market_generation":true,
            "bot":{"name":name,"personality":personality,"style_seed":name.bytes().fold(0u32, |h,b| h.wrapping_mul(31).wrapping_add(u32::from(b)))},
            "immutable_facts":{"give":give,"want":want},"message":"Generate a trade speech template in my own voice, using only the given grammar."});
        tokio::spawn(async move {
            let start = Instant::now();
            let id = format!("market-{:016x}", rand::random::<u64>());
            let reply = service.ask(&payload, &id).await.and_then(|reply| {
                if reply.intent != intent {
                    return Err(Failure("invalid_market_wording"));
                }
                render(&reply.reply, intent, &give, &want, &name).map(|text| Reply {
                    intent,
                    reply: text,
                })
            });
            service.completed(&reply, start.elapsed());
            if let Err(error) = &reply {
                tracing::error!(request=id, bot=name, %error, "market generation failed; no substitute speech");
            }
            *result.lock().expect("market speech result poisoned") = Some(reply.map(|r| r.reply));
        });
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generated_market_templates_cannot_change_inventory_prices_or_consent() {
        for text in [
            "WTS {give} FOR {want}",
            "selling {give} = {want} @{name}",
            "WTB {want} FOR {give}",
        ] {
            let generated =
                render(text, Intent::MarketAdvertise, "4 spd", "1 def", "Trader").unwrap();
            assert!(generated.contains("4 spd") && generated.contains("1 def"));
        }
        assert_eq!(
            render(
                "nah need {want}",
                Intent::MarketRefuse,
                "",
                "1 def",
                "Trader"
            )
            .unwrap(),
            "nah need 1 def"
        );
        for text in [
            "wts 4 spd for 1 def",
            "wts {give} for 6 life",
            "wts {give} for {want} free",
            "agreed {give} for {want}",
            "wts {give} for {give}",
        ] {
            assert!(
                render(text, Intent::MarketAdvertise, "4 spd", "1 def", "Trader").is_err(),
                "{text}"
            );
        }
    }
}

#[cfg(test)]
mod provider_tests {
    use super::*;
    use crate::bots::pursuits::market::{Channel, Message};
    async fn generated(
        service: &Arc<Service>,
        name: &str,
        message: &Message,
        c: &Content,
    ) -> Result<String> {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let Some(text) =
                    service.market_wording(name, message, c, json!({"voice":"terse"}))?
                {
                    return Ok(text);
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("generation timed out")
    }
    fn envelope(text: &str) -> Value {
        json!({"model":MODEL,"choices":[{"finish_reason":"tool_calls","message":{
            "provider_metadata":{"gateway":{"routing":{"canonicalSlug":MODEL,"finalProvider":"nvidia"},"cost":"0"}},
            "tool_calls":[{"type":"function","function":{"name":"reply_to_player","arguments":json!({"intent":"market_advertise","reply":text}).to_string()}}]}}]})
    }
    #[tokio::test]
    async fn market_generation_reuses_success_for_same_truthful_goods_and_observes_provider_failure()
     {
        let c = Content::load(std::path::Path::new("data")).unwrap();
        let message = Message {
            text: "wts 4 spd for 1 def".into(),
            channel: Channel::Local,
        };
        let (service, server) = super::super::tests::provider(
            axum::http::StatusCode::OK,
            envelope("SELLING {give} FOR {want} @{name}"),
        )
        .await;
        assert_eq!(
            generated(&service, "Trader", &message, &c).await.unwrap(),
            "SELLING 4 spd FOR 1 def @Trader"
        );
        assert_eq!(
            generated(&service, "Trader", &message, &c).await.unwrap(),
            "SELLING 4 spd FOR 1 def @Trader"
        );
        assert_eq!(service.health()["daily_requests"], 1);
        assert_eq!(
            generated(&service, "Other", &message, &c).await.unwrap(),
            "SELLING 4 spd FOR 1 def @Other"
        );
        assert_eq!(service.health()["daily_requests"], 2);
        server.abort();
        let (service, server) =
            super::super::tests::provider(axum::http::StatusCode::SERVICE_UNAVAILABLE, json!({}))
                .await;
        assert!(generated(&service, "Trader", &message, &c).await.is_err());
        assert_eq!(service.health()["failures"], 1);
        assert_eq!(service.health()["successes"], 0);
        assert_eq!(service.health()["last_error"], "provider_rejected");
        assert!(
            service
                .market_wording("Trader", &message, &c, json!({"voice":"terse"}))
                .is_err()
        );
        service
            .wordings
            .lock()
            .unwrap()
            .get_mut("Trader:MarketAdvertise")
            .unwrap()
            .created = Instant::now() - Duration::from_secs(61);
        assert!(generated(&service, "Trader", &message, &c).await.is_err());
        assert_eq!(service.health()["daily_requests"], 2);
        assert_eq!(service.health()["successes"], 0);
        server.abort();
    }
    #[tokio::test]
    async fn market_generation_blocks_changed_stock_terms_and_model_invented_prices() {
        let c = Content::load(std::path::Path::new("data")).unwrap();
        let (service, server) = super::super::tests::provider(
            axum::http::StatusCode::OK,
            envelope("wts {give} for {want}"),
        )
        .await;
        let old = Message {
            text: "wts 4 spd for 1 def".into(),
            channel: Channel::Local,
        };
        let changed = Message {
            text: "wts 5 spd for 1 def".into(),
            ..old.clone()
        };
        assert!(
            service
                .market_wording("Trader", &old, &c, json!({"voice":"terse"}))
                .unwrap()
                .is_none()
        );
        assert!(
            service
                .market_wording("Trader", &changed, &c, json!({"voice":"terse"}))
                .unwrap()
                .is_none()
        );
        assert_eq!(
            generated(&service, "Trader", &changed, &c).await.unwrap(),
            "wts 5 spd for 1 def"
        );
        assert_eq!(service.health()["daily_requests"], 2);
        server.abort();
        let (service, server) = super::super::tests::provider(
            axum::http::StatusCode::OK,
            envelope("wts {give} for 9 life"),
        )
        .await;
        assert!(generated(&service, "Trader", &old, &c).await.is_err());
        assert_eq!(service.health()["last_error"], "invalid_market_wording");
        assert_eq!(service.health()["successes"], 0);
        server.abort();
    }
    #[tokio::test]
    async fn market_pending_generation_loses_publication_authority_when_real_stock_is_lost() {
        use crate::bots::pursuits::market::Market;
        let (c, mut v) = crate::bots::quest_tests::fixture();
        v.map.name = "Nexus".into();
        v.session.world = -2;
        v.character.items[4..8].fill(0xa21);
        let mut market = Market::default();
        market.name = "Trader".into();
        let grid = Grid::terrain(&v);
        for _ in 0..32 {
            let d = market
                .decide(
                    &v,
                    &c,
                    None,
                    &grid,
                    &mut rand::rngs::mock::StepRng::new(0, 1),
                )
                .unwrap()
                .unwrap();
            if market.outgoing.is_some() {
                break;
            }
            v.position = d.movement.expect("market position should be reachable");
        }
        let ad = market.outgoing.clone().unwrap();
        assert!(market.message_current(&ad, &v, &c, None).unwrap());
        let (service, server) = super::super::tests::provider(
            axum::http::StatusCode::OK,
            envelope("wts {give} for {want}"),
        )
        .await;
        assert!(
            service
                .market_wording("Trader", &ad, &c, json!({}))
                .unwrap()
                .is_none()
        );
        v.character.items[4] = -1;
        v.character.inventory_revision += 1;
        assert!(!market.message_current(&ad, &v, &c, None).unwrap());
        assert_eq!(
            generated(&service, "Trader", &ad, &c).await.unwrap(),
            "wts 4 spd for 1 def"
        );
        assert!(
            !market.message_current(&ad, &v, &c, None).unwrap(),
            "a valid model reply cannot restore lost stock or publication authority"
        );
        server.abort();
    }
}
