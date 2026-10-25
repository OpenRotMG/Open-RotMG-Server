use super::*;
use crate::server::App;
use rand::seq::SliceRandom;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

#[cfg(test)]
mod tests;

pub(super) struct Departure {
    pub profile: Profile,
    pub account: i32,
    pub retire: bool,
}

async fn fill(
    app: &App,
    done: &mpsc::Sender<Departure>,
    online: &mut BTreeSet<&'static str>,
    resting: &BTreeMap<&'static str, Instant>,
) -> Result<()> {
    let mut candidates = persona::PERSONAS.clone();
    candidates.shuffle(&mut rand::thread_rng());
    for profile in candidates {
        if online.len() == app.bot_population {
            break;
        }
        if online.contains(profile.identity)
            || resting
                .get(profile.identity)
                .is_some_and(|until| *until > app.world.clock.now())
        {
            continue;
        }
        if let Some(account) = app.store.shore_account(profile.identity.into()).await?
            && app.world.supervisor.account_fenced(account)?
        {
            tracing::error!(
                account,
                name = profile.name,
                "fenced persona remains offline pending inspection"
            );
            continue;
        }
        let character = match runtime::persona_character(app, profile).await {
            Ok(character) => character,
            Err(error)
                if error
                    .downcast_ref::<crate::store::Rejected>()
                    .is_some_and(|r| r.0 == "persona name is already in use") =>
            {
                tracing::info!(
                    name = profile.name,
                    "persona name belongs to another account; not impersonating it"
                );
                continue;
            }
            Err(error) => {
                return Err(error).with_context(|| format!("provision persona {}", profile.name));
            }
        };
        let account = character.account;
        let Some(owner) = app.try_claim(account)? else {
            tracing::warn!(
                online = online.len(),
                target = app.bot_population,
                "population below target: player admission capacity occupied"
            );
            break;
        };
        // Logins begin in Nexus; the bot must choose and enter a published Realm portal.
        runtime::launch(
            app,
            profile,
            character,
            (-2, None),
            None,
            owner,
            Some(done.clone()),
        )
        .await?;
        online.insert(profile.identity);
        tracing::info!(
            account,
            name = profile.name,
            online = online.len(),
            target = app.bot_population,
            "inhabitant logged in"
        );
    }
    Ok(())
}

pub(super) async fn start(app: &App) -> Result<()> {
    let (done, mut departed) = mpsc::channel(app.bot_population.max(1));
    let mut online = BTreeSet::new();
    let mut resting = BTreeMap::new();
    fill(app, &done, &mut online, &resting).await?;
    let app = app.clone();
    let mut shutdown = app.shutdown.subscribe();
    let supervisor = app.world.supervisor.clone();
    supervisor.spawn("bot-population", async move {
        let mut poll = tokio::time::interval(Duration::from_secs(30));
        loop {
            tokio::select! {
                _ = shutdown.changed() => return Ok(()),
                _ = poll.tick() => {},
                Some(exit) = departed.recv() => {
                    online.remove(exit.profile.identity);
                    if exit.retire {
                        app.store.retire_persona(exit.profile.identity.into(), exit.account).await?;
                    }
                    resting.insert(exit.profile.identity, app.world.clock.now() + Duration::from_secs(20 * 60));
                    tracing::info!(name=exit.profile.name,account=exit.account,retired=exit.retire,online=online.len(),"inhabitant session ended");
                }
            }
            if *shutdown.borrow() { return Ok(()); }
            if app.world.clock.paused() { continue; }
            fill(&app, &done, &mut online, &resting).await?;
            if online.len() < app.bot_population {
                tracing::warn!(online=online.len(),target=app.bot_population,"population below target; admission or eligible personas unavailable");
            }
        }
    });
    Ok(())
}

pub(super) fn may_finish(brain: &Brain, v: &PlayerView) -> bool {
    !v.lifecycle.busy
        && !brain.mind.social.gift_active()
        && brain.mind.market.incoming.is_none()
        && brain.mind.market.trade.is_none()
        && brain
            .mind
            .market
            .bargain
            .as_ref()
            .is_none_or(|b| b.resolution.is_some())
        && brain.agreement.is_none()
        && brain.group.as_ref().is_none_or(|g| g.finished)
}
