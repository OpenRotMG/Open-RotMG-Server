use super::*;
use crate::{content::Object, store::Purchase};

// Provisional fixed prices: realm-src 1855c017, vendors/MerchantLists.cs; not authenticated 123.5.1 prices.
// The recovered map has stock but no prices. Unlisted merchandise must never inherit a free offer.
fn price(item: &Object) -> Option<(i32, u8)> {
    Some(match item.kind {
        0x705 | 0x706 => (25, 0),
        0x701 | 0x70a | 0x70b | 0x71f | 0xc19 => (50, 0),
        0xc23 => (80, 0),
        // Target-era offer, not later timed boosters: wikiwiki.jp/rmd/version/build123.5.0.
        0xc42 => (100, 0),
        0xc2e => (200, 0),
        0xc11 => (300, 0),
        0x710 => (400, 0),
        0xb3e => (11250, 1),
        // The recovered stock has five doses; retain the comparator's fixed elixir offer.
        0xb16 => (500, 1),
        // Preserved pre-Pet-Yard table: wikiwiki.jp/rmd/old/ペット and アイテム売り場.
        0x1513 => (190, 0),
        0x151b | 0x151e => (490, 0),
        0xa50 => (35, 1),
        0xa51 => (50, 1),
        0xb0d => (150, 1),
        0xb0f => (100, 1),
        // Preserved old-shop table: https://wikiwiki.jp/rmd/アイテム売り場#DrakeEgg
        0xaa9 | 0xaf3 => (190, 1),
        0xaf1 => (180, 1),
        0xaef => (690, 1),
        _ if item.class == "Dye" => (if item.name.contains("Cloth") { 120 } else { 60 }, 0),
        _ => return None,
    })
}

fn value(e: &Entity, stat: u8) -> Result<i32> {
    e.extra_stats
        .iter()
        .find(|(id, _)| *id == stat)
        .map(|(_, value)| *value)
        .with_context(|| format!("merchant {} missing stat {stat}", e.id))
}

pub(super) fn configure(e: &mut Entity, content: &Content) -> Result<()> {
    let item = content.get(u16::try_from(value(e, 34)?)?)?;
    ensure!(
        matches!(item.class.as_str(), "Equipment" | "Dye"),
        "merchant stock is not an item"
    );
    ensure!(value(e, 42)? >= 0, "invalid merchant stock count");
    if let Some((price, currency)) = price(item) {
        e.extra_stats
            .extend([(36, price), (40, i32::from(currency))]);
    } else {
        tracing::warn!(merchant=e.id, item=item.kind, name=%item.name, "merchant unavailable: no sourced shop price configured");
    }
    // The static stock does not rotate; don't advertise the recovered snapshot's expired countdown.
    e.extra_stats.retain(|(id, _)| *id != 43);
    e.extra_stats.push((43, -1));
    if !e.extra_stats.iter().any(|(id, _)| *id == 45) {
        e.extra_stats.push((45, 0));
    }
    Ok(())
}

pub(crate) fn reply(success: bool, message: &str) -> Packet {
    let mut out = Writer::new();
    out.i32(i32::from(!success)).utf(message);
    Packet {
        id: wire::BUY_RESULT,
        body: out.0,
    }
}

impl World {
    pub fn buy(&mut self, id: i32, merchant: i32) -> Result<InventoryPlan> {
        let p = self.players.get(&id).context("purchase owner missing")?;
        ensure!(
            p.active() && !p.pending && !p.paused && p.character.hp > 0,
            "Cannot buy while busy, paused or dead."
        );
        let e = self
            .entities
            .get(&merchant)
            .context("Merchant no longer exists.")?;
        ensure!(
            self.content.get(e.kind)?.class == "Merchant"
                && e.accessible(p.character.account)
                && p.known.contains(&merchant)
                && p.pos.distance(e.pos) < 1.5,
            "Move closer to the merchant."
        );
        ensure!(!e.reserved, "This merchant is processing another purchase.");
        ensure!(value(e, 42)? > 0, "This merchant is sold out.");
        ensure!(
            p.presentation.stars() >= value(e, 45)?,
            "Insufficient star rank."
        );
        let item = u16::try_from(value(e, 34)?)?;
        let price = value(e, 36).context("This item has no configured shop price.")?;
        let currency = u8::try_from(value(e, 40)?)?;
        ensure!(
            price > 0 && currency <= 1,
            "Merchant has an invalid price or currency."
        );
        let slot = (4..12)
            .find(|slot| p.character.items[*slot] == -1)
            .context("Not enough inventory space.")?;
        let mut items = p.character.items.clone();
        items[slot] = i32::from(item);
        self.cancel_trade(id, "Trade cancelled by purchase.");
        let mut plan = self.plan(id, items, None);
        plan.purchase = Some((
            merchant,
            Purchase {
                item,
                slot,
                price,
                currency,
            },
        ));
        self.players.get_mut(&id).unwrap().pending = true;
        self.entities.get_mut(&merchant).unwrap().reserved = true;
        Ok(plan)
    }

    pub fn finish_purchase(
        &mut self,
        plan: InventoryPlan,
        result: Result<Option<(i32, i32)>>,
    ) -> Result<()> {
        let (merchant, purchase) = plan
            .purchase
            .as_ref()
            .context("purchase reservation missing")?;
        let merchant = *merchant;
        let p = self
            .players
            .get(&plan.player)
            .context("purchase owner disappeared")?;
        ensure!(
            p.pending
                && p.character.revision == plan.revision
                && p.character.account == plan.account
                && p.character.id == plan.character,
            "stale or foreign purchase completion"
        );
        let e = self
            .entities
            .get(&merchant)
            .context("reserved merchant disappeared")?;
        ensure!(
            e.reserved
                && value(e, 42)? > 0
                && value(e, 34)? == i32::from(purchase.item)
                && value(e, 36)? == purchase.price
                && value(e, 40)? == i32::from(purchase.currency),
            "merchant reservation changed"
        );
        let committed = self.committed(plan.player, result)?;
        let Some((revision, balance)) = committed else {
            self.entities.get_mut(&merchant).unwrap().reserved = false;
            let p = self.players.get_mut(&plan.player).unwrap();
            p.pending = false;
            let message = if purchase.currency == 0 {
                "Not enough gold."
            } else {
                "Not enough fame."
            };
            let packet = reply(false, message);
            send(p, packet.id, packet.body);
            tracing::info!(
                player = plan.player,
                merchant,
                price = purchase.price,
                currency = purchase.currency,
                "purchase declined: insufficient funds"
            );
            return Ok(());
        };
        let p = self.players.get_mut(&plan.player).unwrap();
        match purchase.currency {
            0 => p.presentation.credits = balance,
            1 => p.presentation.fame = balance,
            _ => unreachable!(),
        }
        let e = self.entities.get_mut(&merchant).unwrap();
        e.reserved = false;
        let (_, stock) = e.extra_stats.iter_mut().find(|(id, _)| *id == 42).unwrap();
        *stock -= 1;
        tracing::info!(
            player = plan.player,
            account = plan.account,
            character = plan.character,
            merchant,
            item = purchase.item,
            price = purchase.price,
            currency = purchase.currency,
            balance,
            stock,
            revision,
            "purchase committed"
        );
        self.finish_inventory(plan, Ok(revision))
    }
}

#[cfg(test)]
mod tests;
