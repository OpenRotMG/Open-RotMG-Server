use super::*;
use crate::server::{App, lifecycle};
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

fn targets(population: usize) -> [usize; 4] {
    // Attendance is tuned, not a census; period views are predominantly blue stars:
    // https://www.youtube.com/watch?v=G08RmJiBdl8&t=30s
    let weights = [1, 4, 4, 1];
    let mut targets = weights.map(|weight| population * weight / 10);
    for _ in targets.iter().sum::<usize>()..population {
        let group = [3, 0, 2, 1]
            .into_iter()
            .max_by_key(|&i| population as i64 * weights[i] as i64 - targets[i] as i64 * 10)
            .expect("population bands exist");
        targets[group] += 1;
    }
    targets
}

fn rank_band(identity: &str, stars: i32, classes: usize) -> usize {
    if identity.starts_with("guest:v1:") {
        0
    } else if stars < classes as i32 {
        1
    } else if stars < 2 * classes as i32 {
        2
    } else {
        3
    }
}

async fn fill(
    app: &App,
    done: &mpsc::Sender<Departure>,
    online: &mut BTreeMap<&'static str, i32>,
    resting: &BTreeMap<&'static str, Instant>,
) -> Result<()> {
    if online.len() >= app.bot_population {
        return Ok(());
    }
    let targets = targets(app.bot_population);
    let classes = app.content.classes.len();
    let mut counts = [0usize; 4];
    for (&identity, &account) in online.iter() {
        ensure!(
            !app.world.supervisor.account_fenced(account)?,
            "fenced account remains in active population"
        );
        counts[rank_band(identity, app.store.account_stars(account).await?, classes)] += 1;
    }
    let mut profiles: Vec<_> = persona::PERSONAS
        .iter()
        .chain(persona::GUESTS.iter())
        .copied()
        .collect();
    profiles.shuffle(&mut rand::thread_rng());
    let mut candidates = Vec::new();
    for profile in profiles {
        if online.contains_key(profile.identity)
            || resting
                .get(profile.identity)
                .is_some_and(|until| *until > app.world.clock.now())
        {
            continue;
        }
        let stars = if let Some(account) = app.store.shore_account(profile.identity.into()).await? {
            if app.world.supervisor.account_fenced(account)? {
                tracing::error!(
                    account,
                    name = profile.name,
                    "fenced persona remains offline pending inspection"
                );
                continue;
            }
            app.store.account_stars(account).await?
        } else {
            profile.stars
        };
        candidates.push((profile, rank_band(profile.identity, stars, classes)));
    }
    while online.len() < app.bot_population {
        let Some(index) = candidates
            .iter()
            .enumerate()
            .filter(|(_, (_, band))| counts[*band] < targets[*band])
            .max_by_key(|(_, (_, band))| targets[*band] - counts[*band])
            .map(|(index, _)| index)
        else {
            break;
        };
        let (profile, _) = candidates.swap_remove(index);
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
        let band = rank_band(
            profile.identity,
            app.store.account_stars(account).await?,
            classes,
        );
        if counts[band] >= targets[band] {
            tracing::info!(
                account,
                name = character.name,
                band,
                ?counts,
                ?targets,
                "persona rank changed before admission; remaining offline"
            );
            continue;
        }
        let Some(owner) = app.try_claim(account)? else {
            tracing::warn!(
                online = online.len(),
                target = app.bot_population,
                "population below target: player admission capacity occupied"
            );
            break;
        };
        // Logins begin in Nexus; the bot must choose and enter a published Realm portal.
        if let Err(error) = runtime::launch(
            app,
            profile,
            character,
            (-2, None),
            None,
            owner,
            Some(done.clone()),
        )
        .await
        {
            if error
                .downcast_ref::<crate::store::Rejected>()
                .is_some_and(|r| r.0 == "persona name is already in use")
            {
                tracing::info!(
                    name = profile.name,
                    "persona name claimed during admission; remaining offline"
                );
                continue;
            }
            return Err(error);
        }
        online.insert(profile.identity, account);
        counts[band] += 1;
        tracing::info!(
            account,
            name = profile.name,
            band,
            ?counts,
            ?targets,
            online = online.len(),
            target = app.bot_population,
            "inhabitant logged in"
        );
    }
    if counts != targets {
        tracing::warn!(
            ?counts,
            ?targets,
            "population rank targets unfilled; no substitution across bands"
        );
    }
    Ok(())
}

async fn maintain(
    app: &App,
    done: &mpsc::Sender<Departure>,
    departed: &mut mpsc::Receiver<Departure>,
    online: &mut BTreeMap<&'static str, i32>,
    resting: &mut BTreeMap<&'static str, Instant>,
) -> Result<()> {
    let stopping = lifecycle::drain::stopping(app);
    if !lifecycle::drain::open(app) || app.world.clock.paused() {
        return Ok(());
    }
    // Provisioning and retirement write durable state; preparation must wait for this turn too.
    let result = async {
        lifecycle::drain::entry(app)?
            .scope(async {
                loop {
                    let exit = match departed.try_recv() {
                        Ok(exit) => exit,
                        Err(mpsc::error::TryRecvError::Empty) => break,
                        Err(mpsc::error::TryRecvError::Disconnected) => {
                            anyhow::bail!("population departure channel closed")
                        }
                    };
                    if exit.retire {
                        app.store
                            .retire_persona(exit.profile.identity.into(), exit.account)
                            .await?;
                    }
                    online.remove(exit.profile.identity);
                    resting.insert(
                        exit.profile.identity,
                        app.world.clock.now() + Duration::from_secs(20 * 60),
                    );
                    tracing::info!(
                        name = exit.profile.name,
                        account = exit.account,
                        retired = exit.retire,
                        online = online.len(),
                        "inhabitant session ended"
                    );
                }
                fill(app, done, online, resting).await
            })
            .await
    }
    .await;
    if let Err(error) = result {
        if error.downcast_ref::<lifecycle::CloseReason>()
            == Some(&lifecycle::CloseReason::Controller)
            && stopping.has_changed()?
        {
            tracing::info!("population work interrupted by lifecycle preparation");
            return Ok(());
        }
        return Err(error);
    }
    if online.len() < app.bot_population {
        tracing::warn!(
            online = online.len(),
            target = app.bot_population,
            "population below target; admission or eligible personas unavailable"
        );
    }
    Ok(())
}

pub(super) async fn start(app: &App) -> Result<()> {
    let (done, mut departed) = mpsc::channel(app.bot_population.max(1));
    let mut online = BTreeMap::new();
    let mut resting = BTreeMap::new();
    maintain(app, &done, &mut departed, &mut online, &mut resting).await?;
    let app = app.clone();
    let mut shutdown = app.shutdown.subscribe();
    let supervisor = app.world.supervisor.clone();
    supervisor.spawn_finite_on(
        &tokio::runtime::Handle::current(),
        "bot-population",
        async move {
            let mut poll = tokio::time::interval(Duration::from_secs(30));
            loop {
                tokio::select! {
                    _ = shutdown.changed() => return Ok(()),
                    _ = poll.tick() => {},
                }
                if *shutdown.borrow() {
                    return Ok(());
                }
                maintain(&app, &done, &mut departed, &mut online, &mut resting).await?;
            }
        },
    );
    Ok(())
}

pub(super) fn may_finish(app: &App, brain: &mut Brain, v: &PlayerView) -> bool {
    let mut society = app.society.lock().expect("adventure registry poisoned");
    brain.group = society.group(v.session.account, v.elapsed_ms);
    brain.offered = society.outgoing(v.session.account, v.elapsed_ms);
    can_finish(brain, v)
}

fn can_finish(brain: &Brain, v: &PlayerView) -> bool {
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
        && (brain.companion.is_none() || brain.invitation_until <= v.time_ms)
        && brain
            .offered
            .as_ref()
            .is_none_or(|offer| offer.until <= v.elapsed_ms)
        && brain.group.as_ref().is_none_or(|g| g.finished)
}
