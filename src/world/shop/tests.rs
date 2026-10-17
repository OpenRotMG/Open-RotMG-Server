use super::*;
use crate::content::WIZARD;

fn fixture() -> (World, i32, i32, mpsc::Receiver<Packet>) {
    let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
    let map = Arc::new(Map::load(std::path::Path::new("data/nexus.wmap"), &content).unwrap());
    let mut w = World::new(content, map).unwrap();
    let merchant = w
        .entities
        .values()
        .find(|e| e.kind == 0x1ca && value(e, 34).unwrap() == 0x705)
        .unwrap()
        .id;
    let (packets, rx) = mpsc::channel(64);
    let (lost, _) = watch::channel(false);
    let id = w
        .enter(super::super::tests::ch(&w), Some(Output { packets, lost }))
        .unwrap();
    let pos = w.entities[&merchant].pos;
    let p = w.players.get_mut(&id).unwrap();
    p.pos = Pos {
        x: pos.x,
        y: pos.y + 0.5,
    };
    p.known.insert(merchant);
    (w, id, merchant, rx)
}

#[test]
fn shop_publishes_prices_and_only_committed_purchases_change_inventory_wallet_and_stock() {
    let (mut w, id, merchant, mut rx) = fixture();
    assert_eq!(
        (
            value(&w.entities[&merchant], 36).unwrap(),
            value(&w.entities[&merchant], 40).unwrap()
        ),
        (25, 0)
    );
    for e in w.entities.values().filter(|e| e.kind == 0x1ca) {
        if price(
            w.content
                .get(u16::try_from(value(e, 34).unwrap()).unwrap())
                .unwrap(),
        )
        .is_some()
        {
            assert!(value(e, 36).unwrap() > 0);
            assert!((0..=1).contains(&value(e, 40).unwrap()));
        } else {
            assert!(value(e, 36).is_err());
        }
        assert_eq!(value(e, 43).unwrap(), -1);
    }
    let status = w.stats(merchant, id).unwrap();
    let mut r = wire::Reader::new(&status);
    r.i32().unwrap();
    r.f32().unwrap();
    r.f32().unwrap();
    let mut stats = BTreeMap::new();
    for _ in 0..r.i16().unwrap() {
        let key = r.u8().unwrap();
        assert!(stats.insert(key, r.i32().unwrap()).is_none());
    }
    r.done().unwrap();
    assert_eq!((stats[&36], stats[&40], stats[&45]), (25, 0, 0));
    // Stat 55 casts to NameChanger in the selected client and aborts Merchant creation.
    assert!(!stats.contains_key(&55));
    let items = w.players[&id].character.items.clone();
    let count = value(&w.entities[&merchant], 42).unwrap();
    let plan = w.buy(id, merchant).unwrap();
    assert_eq!(w.players[&id].character.items, items);
    assert_eq!(w.players[&id].presentation.credits, 100);
    assert_eq!(value(&w.entities[&merchant], 42).unwrap(), count);
    assert!(w.buy(id, merchant).is_err());
    assert!(
        w.swap(
            id,
            Slot {
                object: id,
                slot: 0,
                kind: items[0]
            },
            Slot {
                object: id,
                slot: 5,
                kind: -1
            }
        )
        .is_err()
    );
    w.finish_purchase(plan, Ok(None)).unwrap();
    assert!(!w.players[&id].pending && !w.entities[&merchant].reserved);
    assert_eq!(w.players[&id].character.items, items);
    assert_eq!(value(&w.entities[&merchant], 42).unwrap(), count);
    let plan = w.buy(id, merchant).unwrap();
    let slot = plan.purchase.as_ref().unwrap().1.slot;
    w.finish_purchase(plan, Ok(Some((1, 75)))).unwrap();
    assert_eq!(w.players[&id].character.items[slot], 0x705);
    assert_eq!(w.players[&id].character.revision, 1);
    assert_eq!(w.players[&id].presentation.credits, 75);
    assert_eq!(value(&w.entities[&merchant], 42).unwrap(), count - 1);
    let replies: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok())
        .filter(|p| p.id == wire::BUY_RESULT)
        .collect();
    assert_eq!(replies.len(), 2);
    let mut r = wire::Reader::new(&replies[1].body);
    assert_eq!(r.i32().unwrap(), 0);
    assert_eq!(r.utf().unwrap(), "Item purchased!");
    r.done().unwrap();
}

#[test]
fn shop_rejects_remote_unseen_nonmerchant_full_dead_paused_ranked_and_sold_out_offers() {
    for failure in 0..9 {
        let (mut w, id, merchant, _) = fixture();
        match failure {
            0 => w.players.get_mut(&id).unwrap().pos.x += 3.,
            1 => {
                w.players.get_mut(&id).unwrap().known.remove(&merchant);
            }
            2 => w.entities.get_mut(&merchant).unwrap().kind = 0x120,
            3 => w.players.get_mut(&id).unwrap().character.items[4..].fill(0xa50),
            4 => w.players.get_mut(&id).unwrap().death_pending = true,
            5 => w.players.get_mut(&id).unwrap().paused = true,
            6 => {
                w.entities
                    .get_mut(&merchant)
                    .unwrap()
                    .extra_stats
                    .iter_mut()
                    .find(|(k, _)| *k == 45)
                    .unwrap()
                    .1 = 1
            }
            7 => {
                w.entities
                    .get_mut(&merchant)
                    .unwrap()
                    .extra_stats
                    .iter_mut()
                    .find(|(k, _)| *k == 42)
                    .unwrap()
                    .1 = 0
            }
            8 => w.entities.get_mut(&merchant).unwrap().reserved = true,
            _ => unreachable!(),
        }
        assert!(w.buy(id, merchant).is_err(), "case {failure}");
        assert_eq!(w.players[&id].presentation.credits, 100);
        assert!(!w.players[&id].pending);
    }
}

#[test]
fn shop_failed_storage_quarantines_without_publishing_or_releasing_uncertain_stock() {
    let (mut w, id, merchant, _) = fixture();
    let before = w.players[&id].character.items.clone();
    let plan = w.buy(id, merchant).unwrap();
    assert!(
        w.finish_purchase(plan, Err(anyhow::anyhow!("lost commit result")))
            .is_err()
    );
    assert_eq!(w.players[&id].character.items, before);
    assert_eq!(w.players[&id].presentation.credits, 100);
    assert!(w.players[&id].pending && w.entities[&merchant].reserved);
    assert!(!w.players[&id].active());
    assert!(price(w.content.get(WIZARD).unwrap()).is_none());
    let (mut w, id, _, _) = fixture();
    let e = w
        .entities
        .values()
        .find(|e| e.kind == 0x1ca && value(e, 34).unwrap() == 0xb16)
        .unwrap();
    let (merchant, pos) = (e.id, e.pos);
    let p = w.players.get_mut(&id).unwrap();
    p.pos = pos;
    p.known.insert(merchant);
    assert!(
        w.buy(id, merchant)
            .err()
            .unwrap()
            .to_string()
            .contains("no configured shop price")
    );
    assert!(!w.players[&id].pending && !w.entities[&merchant].reserved);
}
