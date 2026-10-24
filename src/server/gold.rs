use super::*;

pub(super) async fn sync(a: &App, account: i32) -> Result<()> {
    // External grants must not publish an old wallet snapshot over a completed shop debit.
    let _gold = a.gold_updates.lock().await;
    let gold = a.store.gold(account).await?;
    let rewards = gold.rewards.clone();
    let delivered = a
        .world
        .directory(move |d| d.sync_gold(account, &gold))
        .await?;
    if delivered {
        a.store.gold_notified(account, rewards).await?;
        tracing::info!(account, "gold reward chat queued");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn class_quest_gold_live_progress_notifies_once_after_commit() {
        let (_dir, a) = account_actions::tests::fixture();
        a.world.clock.set_paused(true);
        let account = a.store.list("rank-gold-live".into()).await.unwrap().id;
        for class in a.content.classes.keys().filter(|&&class| class != WIZARD) {
            let mut ch = a
                .store
                .create_prepared(account, a.content.get(*class).unwrap(), None)
                .await
                .unwrap();
            ch.fame = 20;
            a.store.progress(ch.clone()).await.unwrap();
            a.store.delete_character(account, ch.id).await.unwrap();
        }
        let mut ch = a
            .store
            .create(account, a.content.get(WIZARD).unwrap())
            .await
            .unwrap();
        ch.level = 20;
        ch.exp = 19_999;
        ch.fame = 19;
        a.store.progress(ch.clone()).await.unwrap();
        let presentation = a.store.presentation(account).await.unwrap();
        assert_eq!(presentation.stars(), 12);
        assert_eq!(presentation.credits, 100);
        let _owner = a.claim_session(account).unwrap();
        let bind = lifecycle::binding(&a, account).unwrap();
        let (packets, mut received) = tokio::sync::mpsc::channel(512);
        let (lost, _lost_rx) = tokio::sync::watch::channel(false);
        let id = a
            .world
            .call(move |w| {
                let id = w.enter_at(
                    ch,
                    Some(crate::world::Output { packets, lost }),
                    None,
                    presentation,
                )?;
                bind(id)?;
                w.players
                    .get_mut(&id)
                    .unwrap()
                    .queue_experience(1, None, false)?;
                Ok(id)
            })
            .await
            .unwrap();
        persistence_tick(&a).await.unwrap();
        let mut messages = Vec::new();
        while let Ok(packet) = received.try_recv() {
            if packet
                .body
                .windows(b"75 Gold".len())
                .any(|w| w == b"75 Gold")
            {
                messages.push(packet);
            }
        }
        assert_eq!(
            messages.len(),
            1,
            "live colour crossing must queue the reward notification"
        );
        let gold = a.store.gold(account).await.unwrap();
        assert_eq!(gold.balance, 175);
        assert!(gold.rewards.is_empty());
        assert_eq!(
            a.world.call(move |w| w.class_quest_rank(id)).await.unwrap(),
            1
        );
        persistence_tick(&a).await.unwrap();
        assert!(a.store.gold(account).await.unwrap().rewards.is_empty());
        while let Ok(packet) = received.try_recv() {
            assert!(
                !packet
                    .body
                    .windows(b"75 Gold".len())
                    .any(|w| w == b"75 Gold")
            );
        }
    }
}
