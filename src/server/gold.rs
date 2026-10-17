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
mod tests;
