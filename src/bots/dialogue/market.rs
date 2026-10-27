use super::*;
use crate::bots::pursuits::market::{
    Advertisement, Goods, Message, goods_text, parse_advertisement, parse_goods, parse_quote,
};

type Repertoire = [String; 3];
pub(super) struct Wording {
    created: Instant,
    next: usize,
    result: Arc<Mutex<Option<std::result::Result<Repertoire, Failure>>>>,
}

pub(super) const PROMPT: &str = r#"Write THREE reusable 2012 Realm trade-chat templates in ONE reply_to_player call with the sole allowed intent. Separate them with literal |. Total reply at most 128 characters.
The tokens {give}, {want}, @{name} are LITERAL text. Never substitute items, numbers or real names. Follow the supplied voice across all three; first is the repeated favorite, the other two occasional alternatives. Use distinct shapes, not three capitalization variants.
For market_advertise the FIRST must be a priced exchange: seller opening wts, selling, sell, s>, trading, trade or t> with {give} then for or = then {want}; buyer opening wtb, buying, buy or b> with {want} then for or = then {give}. Spaces after > and around = are optional. Also allowed: {give} = {want}, or s>{give} ({want}).
Alternatives may be priced exchanges or bare selling interest (selling {give}, s>{give}) or bare buying interest (wtb {want}, buying {want}, b>{want}). Bare interest omits the price and is NOT an agreed exchange. Each mentioned goods token exactly once. Optionally end an ad with literal @{name}, then optionally one ! at the very end.
For market_refuse use short refusals: nty, nah, no thx, sry nty, or one of these followed by need {want}. No {give} or name in refusals.
Keep literal brace tokens lowercase, but vary other capitalization, opening, spacing and name signature to fit this speaker's supplied voice. No other words, punctuation, promises or acceptance. Current inventory facts are inserted only by the program. Native trade windows alone authorize acceptance."#;

fn validated_template(template: &str, intent: Intent) -> std::result::Result<bool, Failure> {
    let remaining = template
        .replace("{give}", "")
        .replace("{want}", "")
        .replace("@{name}", "");
    let valid_tokens = !remaining.contains(['{', '}'])
        && (!template.contains('@') || template.contains("@{name}"))
        && ["{give}", "{want}", "@{name}"]
            .iter()
            .all(|t| template.matches(t).count() <= 1);
    let give = Goods {
        kind: 0,
        quantity: 1,
    };
    let want = Goods {
        kind: 1,
        quantity: 1,
    };
    let mut priced = false;
    let valid = valid_tokens
        && match intent {
            Intent::MarketAdvertise => match parse_advertisement(template, |s| match s {
                "{give}" => Some(give.clone()),
                "{want}" => Some(want.clone()),
                _ => None,
            }) {
                Some(Advertisement::Quote(q)) => {
                    priced = true;
                    q == [give, want]
                }
                Some(Advertisement::Selling(g)) => g == give,
                Some(Advertisement::Buying(g)) => g == want,
                None => false,
            },
            Intent::MarketRefuse => {
                let lower = template.to_ascii_lowercase();
                let refusal = lower.strip_suffix(" need {want}").unwrap_or(&lower);
                matches!(refusal, "nty" | "nah" | "no thx" | "sry nty")
            }
            _ => false,
        };
    if !valid {
        return Err(Failure::Error("invalid_market_wording"));
    }
    Ok(priced)
}

fn repertoire(reply: &str, intent: Intent) -> std::result::Result<Repertoire, Failure> {
    let templates: Repertoire = reply
        .split('|')
        .map(|s| s.trim().to_owned())
        .collect::<Vec<_>>()
        .try_into()
        .map_err(|_| Failure::Error("invalid_market_wording"))?;
    let mut seen = BTreeSet::new();
    for (i, template) in templates.iter().enumerate() {
        let priced = validated_template(template, intent)?;
        if !seen.insert(template.to_ascii_lowercase())
            || (i == 0 && intent == Intent::MarketAdvertise && !priced)
        {
            return Err(Failure::Error("invalid_market_wording"));
        }
    }
    Ok(templates)
}

fn render(
    template: &str,
    intent: Intent,
    give: &str,
    want: &str,
    name: &str,
) -> std::result::Result<String, Failure> {
    validated_template(template, intent)?;
    let text = template
        .replace("{give}", give)
        .replace("{want}", want)
        .replace("@{name}", &format!("@{name}"));
    crate::world::chat::validate(&text).map_err(|_| Failure::Error("invalid_market_wording"))?;
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
            let want = message
                .text
                .strip_prefix("nty need ")
                .and_then(|s| parse_goods(s, c))
                .context("invalid market speech intent")?;
            (Intent::MarketRefuse, String::new(), goods_text(&want, c)?)
        };
        let key = format!("{name}:{intent:?}");
        let mut cache = self.wordings.lock().expect("market speech cache poisoned");
        if let Some(entry) = cache.get_mut(&key) {
            let result = entry.result.lock().expect("market speech result poisoned");
            match result.as_ref() {
                Some(Ok(templates)) => {
                    // Keep a recognizable favorite instead of paraphrasing every advertisement.
                    let index = [0, 0, 1, 0, 0, 2][entry.next];
                    let text = render(&templates[index], intent, &give, &want, name)?;
                    entry.next = (entry.next + 1) % 6;
                    return Ok(Some(text));
                }
                Some(Err(error)) if entry.created.elapsed() < Duration::from_secs(60) => {
                    return Err((*error).into());
                }
                Some(Err(_)) => {}
                None => return Ok(None),
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
                created: Instant::now(),
                next: 0,
                result: result.clone(),
            },
        );
        let service = self.clone();
        let name = name.to_owned();
        let seed = persona::seed(&name);
        let voice = [
            "compact s> notation, lowercase",
            "uppercase seller words and a name signature",
            "lowercase buyer words, no signature",
            "item-first equals notation, no signature",
            "lowercase trading words with a name signature",
            "compact b> notation, uppercase",
        ][seed as usize % 6];
        let payload = json!({"allowed_intents":[intent],"automatic":true,"market_generation":true,
            "bot":{"personality":personality,"style_seed":seed,"voice":voice},
            "message":"Generate my favorite and two occasional alternatives in my own consistent voice. Use only the given grammar and literal slots."});
        tokio::spawn(async move {
            let start = Instant::now();
            let id = format!("market-{:016x}", rand::random::<u64>());
            let mut templates = None;
            let reply = service.ask(&payload, &id).await.and_then(|reply| {
                if reply.intent != intent { return Err(Failure::Error("invalid_market_wording")); }
                match repertoire(&reply.reply, intent) {
                    Ok(words) => templates = Some(words),
                    Err(error) => {
                        tracing::error!(request=id, bot=name, ?intent, template=?reply.reply, "provider returned invalid market repertoire");
                        return Err(error);
                    }
                }
                Ok(reply)
            });
            service.completed(&reply, start.elapsed());
            match &reply {
                Err(Failure::LocalCapacity) => tracing::info!(
                    request = id,
                    bot = name,
                    "market generation skipped after local capacity denial; no substitute speech"
                ),
                Err(error) => {
                    tracing::error!(request=id, bot=name, %error, "market generation failed; no substitute speech")
                }
                Ok(_) => tracing::info!(
                    request = id,
                    bot = name,
                    ?intent,
                    ?templates,
                    "market reusable repertoire generated"
                ),
            }
            *result.lock().expect("market speech result poisoned") =
                Some(reply.map(|_| templates.expect("validated market repertoire missing")));
        });
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bots::pursuits::market::Channel;

    fn content() -> Content {
        Content::load(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data")).unwrap()
    }

    #[test]
    fn market_templates_share_human_parser_without_pricing_bare_interest() {
        let c = content();
        let q = parse_quote("wts 5 spd for 1 def", &c).unwrap();
        for template in [
            "s>{give}={want}",
            "BUYING {want} FOR {give} @{name}!",
            "{give} = {want}",
            "s>{give} ({want})",
            "trading {give} {want}",
        ] {
            let text = render(
                template,
                Intent::MarketAdvertise,
                "5 spd",
                "1 def",
                "Trader",
            )
            .unwrap();
            assert_eq!(parse_quote(&text, &c), Some(q.clone()), "{text}");
        }
        for template in ["selling {give}!", "wtb {want}", "b>{want} @{name}"] {
            let text = render(
                template,
                Intent::MarketAdvertise,
                "5 spd",
                "1 def",
                "Trader",
            )
            .unwrap();
            assert!(parse_advertisement(&text, |s| parse_goods(s, &c)).is_some());
            assert_eq!(parse_quote(&text, &c), None);
        }
        for bad in [
            "wtb {give} for {want}",
            "wts {give} for {give}",
            "wts {give} for {want} deal",
            "selling {give} @{name} @Else",
            "selling {give} @Else",
            "selling {GIVE}",
            "ok accepted",
            "nty need {want} deal",
        ] {
            assert!(
                validated_template(bad, Intent::MarketAdvertise).is_err(),
                "{bad}"
            );
            assert!(
                validated_template(bad, Intent::MarketRefuse).is_err(),
                "{bad}"
            );
        }
        for refusal in ["nty", "nah need {want}", "no thx", "sry nty need {want}"] {
            let text = render(refusal, Intent::MarketRefuse, "5 spd", "1 def", "Trader").unwrap();
            assert_eq!(parse_quote(&text, &c), None);
        }
        assert!(
            repertoire(
                "wts {give} for {want}|WTS {give} FOR {want}|selling {give}",
                Intent::MarketAdvertise
            )
            .is_err()
        );
        assert!(repertoire("wtb {want}|s>{give}|{give}={want}", Intent::MarketAdvertise).is_err());
        assert!(repertoire("nty|nah", Intent::MarketRefuse).is_err());
        assert!(
            render(
                "selling {give}",
                Intent::MarketAdvertise,
                &"x".repeat(129),
                "1 def",
                "Trader"
            )
            .is_err()
        );
        for bad in [
            "wtb life",
            "selling ghostly !",
            "wts 0 spd for def",
            "wts 9 spd for def",
            "wts spd=spd",
            "wts spd for def @Trader accepted",
            "accepted 1 def",
            "wts spd for def=life",
        ] {
            assert_eq!(parse_quote(bad, &c), None, "{bad}");
        }
    }

    #[tokio::test]
    async fn market_cache_repeats_favorite_renders_current_slots_and_preserves_failures() {
        let c = content();
        let service =
            Arc::new(Service::new("unused", "http://127.0.0.1:1", Duration::from_secs(1)).unwrap());
        let intent = Intent::MarketAdvertise;
        let key = "Trader:MarketAdvertise".to_owned();
        let words = repertoire(
            "s>{give}={want} @{name}|wtb {want} for {give}|selling {give}",
            intent,
        )
        .unwrap();
        service.wordings.lock().unwrap().insert(
            key.clone(),
            Wording {
                created: Instant::now(),
                next: 0,
                result: Arc::new(Mutex::new(Some(Ok(words.clone())))),
            },
        );
        for index in [0, 0, 1, 0, 0, 2] {
            let message = Message {
                text: "wts 5 spd for 1 def".into(),
                channel: Channel::Local,
            };
            assert_eq!(
                service
                    .market_wording("Trader", &message, &c, json!({}))
                    .unwrap(),
                Some(render(&words[index], intent, "5 spd", "1 def", "Trader").unwrap())
            );
        }
        let changed = Message {
            text: "wts 1 life for 1 hydra".into(),
            channel: Channel::Local,
        };
        assert_eq!(
            service
                .market_wording("Trader", &changed, &c, json!({}))
                .unwrap(),
            Some("s>1 life=1 hydra @Trader".into())
        );
        assert_eq!(service.health()["daily_requests"], 0);
        *service.wordings.lock().unwrap()[&key]
            .result
            .lock()
            .unwrap() = Some(Err(Failure::Error("provider_rejected")));
        assert_eq!(
            service
                .market_wording("Trader", &changed, &c, json!({}))
                .unwrap_err()
                .to_string(),
            "provider_rejected"
        );
        let malformed = Message {
            text: "wtb life".into(),
            channel: Channel::Local,
        };
        assert_eq!(
            service
                .market_wording("Trader", &malformed, &c, json!({}))
                .unwrap_err()
                .to_string(),
            "invalid market speech intent"
        );
    }
}
