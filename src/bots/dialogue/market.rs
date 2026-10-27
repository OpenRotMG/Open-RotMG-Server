use super::*;
use crate::bots::pursuits::market::{
    Advertisement, Goods, Message, counter_body, goods_text, parse_advertisement, parse_goods, parse_quote,
};

type Repertoire = [String; 3];
const FAVORITE_SHAPES: [&str; 8] = [
    "s>{give}={want}",
    "selling {give} for {want}",
    "buying {want} for {give}",
    "{give}={want}",
    "wtb {want} {give}",
    "trading {give} for {want}",
    "s>{give} ({want})",
    "b>{want}={give}",
];
const REFUSAL_SHAPES: [&str; 8] = [
    "nty",
    "nah",
    "no thx",
    "sry nty",
    "nty need {want}",
    "nah need {want}",
    "no thx need {want}",
    "sry nty need {want}",
];
const COUNTER_SHAPES: [&str; 8] = [
    "nty {give} for {want}", "nah {give} for {want}", "can do {give} for {want}",
    "nty {give}={want}", "nah {give}={want}", "can do {give}={want}",
    "no thx {give} for {want}", "sry {give} for {want}",
];
const BANK_SHAPES: [&str; 5] = ["full inv", "inv full", "need room", "making room", "lemme make room"];
const THANKS_SHAPES: [&str; 5] = ["ty", "thx", "thanks", "tyvm", "cheers"];
pub(super) struct Wording {
    created: Instant,
    next: usize,
    result: Arc<Mutex<Option<std::result::Result<Repertoire, Failure>>>>,
}

pub(super) const PROMPT: &str = r#"Write THREE reusable 2012 Realm trade-chat templates in ONE reply_to_player call with the sole allowed intent. Separate them with literal |. Total reply at most 128 characters. Do not add literal quotation marks or markdown inside the reply string; encode tool arguments as normal JSON.
The tokens {give}, {want}, @{name} are LITERAL text. Never substitute items, numbers or real names. Follow the supplied voice across all three; first is the repeated favorite, the other two occasional alternatives. favorite_shape and syntax_examples supply valid literal syntax for the sole allowed intent, not descriptive words to print; adapt only capitalization and a permitted ad name signature. Use distinct shapes, not three capitalization variants.
For market_advertise use the supplied syntax_examples as literal ad grammar; never mix in refusal words such as need, nty or no thx. The FIRST must be a priced exchange: seller opening wts, selling, sell, s>, trading, trade or t> with {give} then for, = or an implicit space-separated price then {want}; buyer opening wtb, buying, buy or b> with {want} then for, = or an implicit space-separated price then {give}. Spaces after > and around = are optional. Also allowed: {give} = {want}, or s>{give} ({want}).
Alternatives may be priced exchanges or bare selling interest (selling {give}, s>{give}) or bare buying interest (wtb {want}, buying {want}, b>{want}). Bare interest omits the price and is NOT an agreed exchange. Each mentioned goods token exactly once. If name_signature is true, optionally end an ad with ONE space followed by literal @{name}, then optionally one ! at the very end. Never attach @{name} directly to a goods token.
For market_refuse use short refusals: nty, nah, no thx, sry nty, or one of these followed by need {want}. No {give} or name in refusals.
For market_counter every template must give the actual complete counterprice: nty, nah, can do, no thx or sry followed by {give} for {want} or {give}={want}. Both goods slots exactly once. For this intent vary only prefix choice and non-token capitalization; keep exactly ONE space after the prefix and exactly the body spacing shown. No ad openings, names, parentheses, other words or trailing punctuation. This is a counteroffer, not agreement.
For market_bank use only the supplied short capacity syntax_examples; no goods, names, promises or outcome. The program selected actual full-inventory preparation.
For market_thanks use only the supplied short acknowledgment syntax_examples; no goods, names, promises or further agreement. The program has verified an exact committed transaction before requesting this wording.
Keep literal brace tokens lowercase, but vary other capitalization, opening, spacing and permitted ad name signature only as allowed for the selected intent to fit this speaker's supplied voice. No other words, punctuation, promises or acceptance. Current inventory facts are inserted only by the program. Native trade windows alone authorize acceptance."#;

fn voice(seed: u64, intent: Intent) -> Value {
    let shapes: &[&str] = match intent {
        Intent::MarketAdvertise => &FAVORITE_SHAPES,
        Intent::MarketRefuse => &REFUSAL_SHAPES,
        Intent::MarketCounter => &COUNTER_SHAPES,
        Intent::MarketBank => &BANK_SHAPES,
        Intent::MarketThanks => &THANKS_SHAPES,
        _ => unreachable!("market voice requires a market intent"),
    };
    json!({
        "favorite_shape": shapes[seed as usize % shapes.len()],
        "syntax_examples": shapes,
        "capitalization": (["lowercase", "UPPERCASE", "initial capital"][(seed / 8) as usize % 3]),
        "name_signature": intent == Intent::MarketAdvertise && (seed / 24).is_multiple_of(2),
    })
}

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
            Intent::MarketCounter => matches!(counter_body(&template.to_ascii_lowercase()),
                Some("{give} for {want}" | "{give}={want}")),
            Intent::MarketBank => BANK_SHAPES.contains(&template.to_ascii_lowercase().as_str()),
            Intent::MarketThanks => THANKS_SHAPES.contains(&template.to_ascii_lowercase().as_str()),
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
    c: &Content,
) -> std::result::Result<String, Failure> {
    validated_template(template, intent)?;
    let text = template
        .replace("{give}", give)
        .replace("{want}", want)
        .replace("@{name}", &format!("@{name}"));
    crate::world::chat::validate(&text).map_err(|_| Failure::Error("invalid_market_wording"))?;
    if intent == Intent::MarketCounter {
        let lower=text.to_ascii_lowercase();
        let actual=counter_body(&lower).and_then(|s| parse_quote(s,c));
        let expected=parse_goods(&give.to_ascii_lowercase(),c).zip(parse_goods(&want.to_ascii_lowercase(),c)).map(|(g,w)| [g,w]);
        if actual.is_none() || actual!=expected { return Err(Failure::Error("invalid_market_wording")); }
    }
    if intent == Intent::MarketAdvertise {
        let give = parse_goods(&give.to_ascii_lowercase(), c);
        let want = parse_goods(&want.to_ascii_lowercase(), c);
        let faithful = match parse_advertisement(&text, |s| parse_goods(s, c)) {
            Some(Advertisement::Quote([g, w])) => Some(g) == give && Some(w) == want,
            Some(Advertisement::Selling(g)) => Some(g) == give,
            Some(Advertisement::Buying(w)) => Some(w) == want,
            None => false,
        };
        if !faithful {
            return Err(Failure::Error("invalid_market_wording"));
        }
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
        let (intent, give, want) = if let Some(q) = message.text.strip_prefix("nty ").and_then(|s| parse_quote(s,c)) {
            (Intent::MarketCounter, goods_text(&q[0],c)?, goods_text(&q[1],c)?)
        } else if message.text=="full inv" {
            (Intent::MarketBank, String::new(), String::new())
        } else if message.text=="ty" {
            (Intent::MarketThanks, String::new(), String::new())
        } else if let Some(q) = parse_quote(&message.text, c) {
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
                    let text = render(&templates[index], intent, &give, &want, name, c)?;
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
        let voice = voice(seed, intent);
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

    #[test]
    fn market_voice_examples_follow_the_allowed_intent() {
        for seed in 0..48 {
            for intent in [Intent::MarketAdvertise, Intent::MarketRefuse] {
                let voice = voice(seed, intent);
                let examples = voice["syntax_examples"].as_array().unwrap();
                assert!(
                    examples
                        .iter()
                        .all(|s| validated_template(s.as_str().unwrap(), intent).is_ok()),
                    "{intent:?}: {voice}"
                );
                assert_eq!(voice["favorite_shape"], examples[seed as usize % 8]);
                assert_eq!(
                    voice["name_signature"],
                    json!(intent == Intent::MarketAdvertise && (seed / 24).is_multiple_of(2))
                );
            }
        }
    }

    #[test]
    fn market_interaction_examples_never_claim_other_intents_or_unbound_facts() {
        for intent in [Intent::MarketCounter,Intent::MarketBank,Intent::MarketThanks] {
            for seed in 0..24 {
                let v=voice(seed,intent);
                assert_eq!(v["name_signature"],false);
                assert!(v["syntax_examples"].as_array().unwrap().iter().all(|s| validated_template(s.as_str().unwrap(),intent).is_ok()));
            }
        }
        for (intent,bad) in [
            (Intent::MarketCounter,"nty {want} for {give}"),
            (Intent::MarketCounter,"nty {give}"),
            (Intent::MarketCounter,"nty {give} for {want} @{name}"),
            (Intent::MarketCounter,"deal {give} for {want}"),
            (Intent::MarketBank,"full inv got {give}"),
            (Intent::MarketBank,"brb will trade later"),
            (Intent::MarketThanks,"thanks for {give}"),
            (Intent::MarketThanks,"trade accepted"),
            (Intent::MarketThanks,"\"thanks\""),
        ] { assert!(validated_template(bad,intent).is_err(),"{intent:?}: {bad}"); }
    }

    #[test]
    fn market_counter_templates_reject_advertisement_only_syntax() {
        for bad in ["nty s>{give} ({want})!","nty selling {give} for {want}","nty {give}={want}!","nty ({give})={want}","nty {give}  for  {want}"] {
            assert!(validated_template(bad,Intent::MarketCounter).is_err(),"{bad}");
            assert!(repertoire(&format!("{bad}|nah {{give}} for {{want}}|can do {{give}}={{want}}"),Intent::MarketCounter).is_err(),"{bad}");
        }
        assert!(repertoire("nty {give} for {want}|nah {give}={want}|can do {give} for {want}",Intent::MarketCounter).is_ok());
    }

    #[tokio::test]
    async fn market_interaction_cache_repeats_favorites_and_renders_only_current_slots() {
        let c=content();
        let service=Arc::new(Service::new("unused","http://127.0.0.1:1",Duration::from_secs(1)).unwrap());
        for (intent,canonical,words) in [
            (Intent::MarketCounter,"nty wts 5 spd for 1 def","nty {give} for {want}|nah {give}={want}|can do {give} for {want}"),
            (Intent::MarketBank,"full inv","full inv|need room|lemme make room"),
            (Intent::MarketThanks,"ty","thx|thanks|tyvm"),
        ] {
            let words=repertoire(words,intent).unwrap();
            service.wordings.lock().unwrap().insert(format!("Trader:{intent:?}"),Wording {
                created:Instant::now(),next:0,result:Arc::new(Mutex::new(Some(Ok(words.clone())))),
            });
            let message=Message { text:canonical.into(),channel:Channel::Tell("Guest1".into()) };
            for i in [0,0,1,0,0,2] {
                assert_eq!(service.market_wording("Trader",&message,&c,json!({})).unwrap(),
                    Some(render(&words[i],intent,"5 spd","1 def","Trader",&c).unwrap()));
            }
        }
        let changed=Message { text:"nty wts 3 spd for 1 def".into(),channel:Channel::Tell("Guest1".into()) };
        assert_eq!(service.market_wording("Trader",&changed,&c,json!({})).unwrap(),Some("nty 3 spd for 1 def".into()));
        assert_eq!(service.health()["daily_requests"],0);
    }

    #[test]
    fn market_favorite_shapes_are_priced_and_invalid_provider_repertoire_stays_rejected() {
        for shape in FAVORITE_SHAPES {
            assert!(validated_template(shape, Intent::MarketAdvertise).unwrap());
        }
        let invalid = "wtf {give} for {want} @{name}|trading words with for {give} = {want} @{name}|s>{give} ({want}) @{name}!";
        assert!(repertoire(invalid, Intent::MarketAdvertise).is_err());
        for bad in invalid.split('|').take(2) {
            assert!(validated_template(bad, Intent::MarketAdvertise).is_err());
        }
        assert!(validated_template("s>{give} ({want}) @{name}!", Intent::MarketAdvertise).unwrap());
        let mixed_refusal =
            "WTB {want} for {give} | s>{give} ({want}) | buying {want}, need {give}";
        assert!(repertoire(mixed_refusal, Intent::MarketAdvertise).is_err());
        for ad in mixed_refusal.split('|').take(2) {
            assert!(validated_template(ad.trim(), Intent::MarketAdvertise).unwrap());
        }
        assert!(validated_template("buying {want}, need {give}", Intent::MarketAdvertise).is_err());
        let attached_name =
            "s>{give} ({want})|selling {give} for {want}|buying {want} for {give}@{name}!";
        assert!(repertoire(attached_name, Intent::MarketAdvertise).is_err());
        for ad in attached_name.split('|').take(2) {
            assert!(validated_template(ad, Intent::MarketAdvertise).unwrap());
        }
        assert!(
            validated_template("buying {want} for {give}@{name}!", Intent::MarketAdvertise)
                .is_err()
        );
        assert!(
            validated_template("buying {want} for {give} @{name}!", Intent::MarketAdvertise)
                .unwrap()
        );
        let ads_for_refusal = "s>{give}={want}|{give}={want}|wtb {want} {give}@{name}!";
        assert!(repertoire(ads_for_refusal, Intent::MarketRefuse).is_err());
    }

    fn content() -> Content {
        Content::load(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data")).unwrap()
    }

    #[test]
    fn market_templates_share_human_parser_without_pricing_bare_interest() {
        let c = content();
        for (give, want) in [
            ("5 spd", "1 def"),
            ("1 life", "1 Robe of the Master"),
            ("1 Robe of the Student", "1 def"),
        ] {
            let q = parse_quote(&format!("wts {give} for {want}"), &c).unwrap();
            for template in [
                "s>{give}={want}",
                "BUYING {want} FOR {give} @{name}!",
                "{give} = {want}",
                "s>{give} ({want})",
                "trading {give} {want}",
            ] {
                let text =
                    render(template, Intent::MarketAdvertise, give, want, "Trader", &c).unwrap();
                assert_eq!(parse_quote(&text, &c), Some(q.clone()), "{text}");
            }
        }
        for template in ["selling {give}!", "wtb {want}", "b>{want} @{name}"] {
            let text = render(
                template,
                Intent::MarketAdvertise,
                "5 spd",
                "1 def",
                "Trader",
                &c,
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
            let text = render(
                refusal,
                Intent::MarketRefuse,
                "5 spd",
                "1 def",
                "Trader",
                &c,
            )
            .unwrap();
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
                "Trader",
                &c
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
                Some(render(&words[index], intent, "5 spd", "1 def", "Trader", &c).unwrap())
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
