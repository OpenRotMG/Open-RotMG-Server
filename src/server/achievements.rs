use super::*;

pub(crate) async fn intentional_update_stop(
    a: &App,
    update: restart::ConsumedUpdate,
) -> Result<()> {
    let operation = update.operation;
    let actors = a
        .world
        .directory(|d| Ok(d.update_achievement_actors()))
        .await?;
    if actors.is_empty() {
        tracing::info!(instance=%operation.instance, owner=%operation.owner, deadline_ms=operation.deadline_ms, "intentional update has no eligible achievement actors");
        return Ok(());
    }
    let accounts = actors.iter().map(|(account, _, _)| *account).collect();
    lifecycle::account(a, accounts, "intentional_update_achievements", move |a| async move {
        lifecycle::stage(Stage::Reservation);
        {
            let owners = a.owners.lock().expect("ownership registry poisoned");
            for (account, avatar, _) in &actors {
                ensure!(owners.get(account).is_some_and(|owner| owner.matches_avatar(*avatar)), "intentional update achievement owner does not bind account={account} avatar={avatar}");
            }
        }
        let snapshots = a.world.directory(move |d| d.update_achievement_characters(&actors)).await?;
        lifecycle::stage(Stage::Store);
        let awards = a.store.achievement_update(snapshots).await?;
        lifecycle::stage(Stage::Publication);
        for (account, balance) in awards {
            tracing::info!(account, balance, amount=100, instance=%operation.instance, owner=%operation.owner, deadline_ms=operation.deadline_ms, "intentional update achievement committed");
            super::gold::sync(&a, account).await?;
        }
        Ok(())
    }).await
}
