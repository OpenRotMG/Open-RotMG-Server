use super::*;
use std::{future::Future, task::Poll};

async fn enter(a: &App) -> (i32, i32, i32, mpsc::Receiver<Packet>) {
    let account = a
        .store
        .register(
            "gold-player".into(),
            "gold@example.invalid".into(),
            "password123".into(),
        )
        .await
        .unwrap();
    let ch = a
        .store
        .create(account, a.content.get(crate::content::WIZARD).unwrap())
        .await
        .unwrap();
    let presentation = a.store.presentation(account).await.unwrap();
    let (packets, rx) = mpsc::channel(128);
    let (lost, _) = tokio::sync::watch::channel(false);
    let (id, merchant) = a
        .world
        .in_world(-2)
        .call(move |w| {
            let e = w
                .entities
                .values()
                .find(|e| e.kind == 0x1ca && e.extra_stats.contains(&(34, 0x705)))
                .unwrap();
            let merchant = e.id;
            let pos = Pos {
                x: e.pos.x,
                y: e.pos.y + 0.5,
            };
            let id = w.enter_at(
                ch,
                Some(crate::world::Output { packets, lost }),
                Some(pos),
                presentation,
            )?;
            w.players.get_mut(&id).unwrap().known.insert(merchant);
            Ok((id, merchant))
        })
        .await
        .unwrap();
    (account, id, merchant, rx)
}
fn messages(rx: &mut mpsc::Receiver<Packet>) -> Vec<String> {
    std::iter::from_fn(|| rx.try_recv().ok())
        .filter(|p| p.id == wire::TEXT)
        .map(|p| {
            let mut r = Reader::new(&p.body);
            r.utf().unwrap();
            r.i32().unwrap();
            r.i32().unwrap();
            r.u8().unwrap();
            r.utf().unwrap();
            let text = r.utf().unwrap();
            r.utf().unwrap();
            r.done().unwrap();
            text
        })
        .collect()
}

async fn balance(a: &App, id: i32) -> i32 {
    let bytes = a
        .world
        .for_player(id)
        .call(move |w| w.stats(id, id))
        .await
        .unwrap();
    let mut r = Reader::new(&bytes);
    r.i32().unwrap();
    r.f32().unwrap();
    r.f32().unwrap();
    let mut balance = None;
    for _ in 0..r.i16().unwrap() {
        let stat = r.u8().unwrap();
        if matches!(stat, 31 | 62) {
            r.utf().unwrap();
        } else {
            let value = r.i32().unwrap();
            if stat == 35 {
                balance = Some(value);
            }
        }
    }
    r.done().unwrap();
    balance.expect("owner gold stat")
}

#[tokio::test]
async fn private_link_updates_online_gold_and_chat_without_reconnect_or_repeat() {
    let (dir, a) = account_actions::tests::fixture();
    let (account, id, _, mut rx) = enter(&a).await;
    let code = a.store.issue_discord_code(account).await.unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(
        "authorization",
        format!("Bearer {}", "a".repeat(64)).parse().unwrap(),
    );
    let response = hiscores::discord_link(
        State(a.clone()),
        ConnectInfo("127.0.0.1:1234".parse().unwrap()),
        headers,
        Json(
            serde_json::from_value::<hiscores::LinkRequest>(
                serde_json::json!({"code":code,"discord_id":"12345678901234567"}),
            )
            .unwrap(),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(balance(&a, id).await, 200);
    assert_eq!(
        messages(&mut rx),
        ["You received 100 Gold for linking your Discord account!"]
    );
    assert!(a.store.gold(account).await.unwrap().rewards.is_empty());
    sync(&a, account).await.unwrap();
    assert!(messages(&mut rx).is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn collected_inventory_commit_updates_online_gold_and_chat() {
    let (dir, a) = account_actions::tests::fixture();
    let (account, id, _, mut rx) = enter(&a).await;
    a.store.compete(account).await.unwrap();
    let mut plan = a
        .world
        .for_player(id)
        .call(move |w| {
            let item = w.players[&id].character.items[4];
            w.swap(
                id,
                Slot {
                    object: id,
                    slot: 4,
                    kind: item,
                },
                Slot {
                    object: id,
                    slot: 5,
                    kind: -1,
                },
            )
        })
        .await
        .unwrap();
    // World pickup tests cover loot provenance; this checks the committed-plan to live-chat boundary.
    plan.collected = Some(0xa22);
    persist(&a, plan).await.unwrap();
    assert_eq!(balance(&a, id).await, 200);
    assert_eq!(a.store.gold(account).await.unwrap().balance, 200);
    assert!(a.store.gold(account).await.unwrap().rewards.is_empty());
    assert_eq!(
        messages(&mut rx),
        ["You received 100 Gold for reaching the top 50% of the Collection Log hiscores!"]
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn offline_rewards_remain_pending_and_full_queues_never_acknowledge_them() {
    let (dir, a) = account_actions::tests::fixture();
    let (account, id, _, _rx) = enter(&a).await;
    let code = a.store.issue_discord_code(account).await.unwrap();
    a.store
        .redeem_discord_code(code, "12345678901234567".into())
        .await
        .unwrap();
    let (packets, mut rx) = mpsc::channel(1);
    packets
        .try_send(Packet {
            id: 0,
            body: vec![],
        })
        .unwrap();
    let (lost, mut watch) = tokio::sync::watch::channel(false);
    a.world
        .for_player(id)
        .call(move |w| {
            w.players.get_mut(&id).unwrap().tx = Some(crate::world::Output { packets, lost });
            Ok(())
        })
        .await
        .unwrap();
    sync(&a, account).await.unwrap();
    assert!(*watch.borrow_and_update());
    assert_eq!(a.store.gold(account).await.unwrap().rewards.len(), 1);
    rx.try_recv().unwrap();
    let (packets, mut rx) = mpsc::channel(128);
    let (lost, _) = tokio::sync::watch::channel(false);
    a.world
        .for_player(id)
        .call(move |w| {
            w.players.get_mut(&id).unwrap().tx = Some(crate::world::Output { packets, lost });
            Ok(())
        })
        .await
        .unwrap();
    sync(&a, account).await.unwrap();
    assert_eq!(messages(&mut rx).len(), 1);
    assert!(a.store.gold(account).await.unwrap().rewards.is_empty());
    a.world
        .for_player(id)
        .call(move |w| {
            w.players.remove(&id);
            Ok(())
        })
        .await
        .unwrap();
    sync(&a, account).await.unwrap();
    assert_eq!(a.store.gold(account).await.unwrap().balance, 200);
    let offline = a
        .store
        .register(
            "offline".into(),
            "offline@example.invalid".into(),
            "password123".into(),
        )
        .await
        .unwrap();
    let code = a.store.issue_discord_code(offline).await.unwrap();
    a.store
        .redeem_discord_code(code, "12345678901234568".into())
        .await
        .unwrap();
    sync(&a, offline).await.unwrap();
    assert_eq!(a.store.gold(offline).await.unwrap().rewards.len(), 1);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn external_gold_refresh_and_shop_debit_are_serialized_in_either_order() {
    for refresh_first in [false, true] {
        let (dir, a) = account_actions::tests::fixture();
        let (account, id, merchant, mut rx) = enter(&a).await;
        let plan = a
            .world
            .for_player(id)
            .call(move |w| w.buy(id, merchant))
            .await
            .unwrap();
        let code = a.store.issue_discord_code(account).await.unwrap();
        a.store
            .redeem_discord_code(code, "12345678901234567".into())
            .await
            .unwrap();
        let gate = a.gold_updates.lock().await;
        let refresh = sync(&a, account);
        let buy = persist(&a, plan);
        tokio::pin!(refresh, buy);
        if refresh_first {
            assert!(
                std::future::poll_fn(|cx| Poll::Ready(refresh.as_mut().poll(cx)))
                    .await
                    .is_pending()
            );
            assert!(
                std::future::poll_fn(|cx| Poll::Ready(buy.as_mut().poll(cx)))
                    .await
                    .is_pending()
            );
        } else {
            assert!(
                std::future::poll_fn(|cx| Poll::Ready(buy.as_mut().poll(cx)))
                    .await
                    .is_pending()
            );
            assert!(
                std::future::poll_fn(|cx| Poll::Ready(refresh.as_mut().poll(cx)))
                    .await
                    .is_pending()
            );
        }
        drop(gate);
        let (b, r) = tokio::join!(buy, refresh);
        b.unwrap();
        r.unwrap();
        assert_eq!(a.store.gold(account).await.unwrap().balance, 175);
        assert_eq!(balance(&a, id).await, 175);
        assert_eq!(messages(&mut rx).len(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
