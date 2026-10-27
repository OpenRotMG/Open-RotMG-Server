use super::*;
use crate::bots::pursuits::market::{Message, goods_text, parse_quote};

pub(super) struct Wording {
    created: Instant,
    result: Arc<Mutex<Option<std::result::Result<String, Failure>>>>,
}

pub(super) const PROMPT: &str = r#"Write a reusable 2012 Realm trade-chat TEMPLATE using reply_to_player once with the sole allowed intent.
The brace tokens are LITERAL text: {give}, {want}, @{name}. Never replace them with items, numbers or a real name.
For market_advertise, examples of valid reply strings are "wts {give} for {want}", "SELLING {give} = {want}", "wtb {want} for {give}" and "s> {give} for {want} @{name}".
Choose a seller opening (wts, selling, sell, s>, trading) followed by {give}, then for or =, then {want}; or a buyer opening (wtb, buying, buy) followed by {want}, then for or =, then {give}. Optionally append literal @{name}.
For market_refuse, reply exactly "nty need {want}", "nah need {want}" or "no thx need {want}", with your own capitalization.
Vary opening and capitalization for the supplied personality/style seed. Keep every required brace token exactly once. No other words, quotes, punctuation, promises or acceptance. Real current inventory facts are inserted by the program only after generation. Native trade windows alone authorize acceptance."#;

fn validated_template_words(
    template: &str,
    intent: Intent,
) -> std::result::Result<Vec<&str>, Failure> {
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
        return Err(Failure::Error("invalid_market_wording"));
    }
    Ok(words)
}

fn render(
    template: &str,
    intent: Intent,
    give: &str,
    want: &str,
    name: &str,
) -> std::result::Result<String, Failure> {
    let mut result = Vec::new();
    for word in validated_template_words(template, intent)? {
        result.push(match word.to_ascii_lowercase().as_str() {
            "{give}" => give.to_owned(),
            "{want}" => want.to_owned(),
            "@{name}" => format!("@{name}"),
            _ => word.to_owned(),
        });
    }
    let text = result.join(" ");
    crate::world::chat::validate(&text).map_err(|_| Failure::Error("invalid_market_wording"))?;
    if text.encode_utf16().count() > 128 {
        return Err(Failure::Error("invalid_market_wording"));
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
            match result.as_ref() {
                Some(Ok(template)) => {
                    return Ok(Some(render(template, intent, &give, &want, name)?));
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
                result: result.clone(),
            },
        );
        let service = self.clone();
        let name = name.to_owned();
        let payload = json!({"allowed_intents":[intent],"automatic":true,"market_generation":true,
            "bot":{"personality":personality,"style_seed":name.bytes().fold(0u32, |h,b| h.wrapping_mul(31).wrapping_add(u32::from(b)))},
            "message":"Generate a trade speech template in my own voice, using only the given grammar."});
        tokio::spawn(async move {
            let start = Instant::now();
            let id = format!("market-{:016x}", rand::random::<u64>());
            let reply = service.ask(&payload, &id).await.and_then(|reply| {
                if reply.intent != intent {
                    return Err(Failure::Error("invalid_market_wording"));
                }
                if let Err(error) = validated_template_words(&reply.reply, intent) {
                    tracing::error!(request=id, bot=name, ?intent, template=?reply.reply, "provider returned invalid market template");
                    return Err(error);
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
                Ok(_) => {}
            }
            *result.lock().expect("market speech result poisoned") = Some(reply.map(|r| r.reply));
        });
        Ok(None)
    }
}
