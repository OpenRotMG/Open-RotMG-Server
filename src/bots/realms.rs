use super::*;
use rand::seq::SliceRandom;

pub(super) fn choose(account: i32, realms: &[(i32, usize)], rng: &mut impl Rng) -> Result<i32> {
    let busiest = realms
        .iter()
        .map(|(_, count)| *count)
        .max()
        .context("no open Realm has capacity")?;
    // Attract bots to crowds without eliminating quieter outings: https://github.com/OpenRotMG/Open-RotMG-Server/issues/365
    let selected = realms
        .choose_weighted(rng, |(_, count)| if *count == busiest { 6u32 } else { 1 })
        .context("invalid Realm crowd preference weights")?
        .0;
    tracing::info!(account, choices = ?realms, busiest, selected, "autonomous Realm crowd preference sampled");
    Ok(selected)
}

fn population(name: &str) -> Result<usize> {
    let (_, counts) = name
        .rsplit_once(" (")
        .context("missing population suffix")?;
    let (count, capacity) = counts
        .strip_suffix(')')
        .and_then(|s| s.split_once('/'))
        .context("expected (population/capacity)")?;
    let count = count.parse::<usize>()?;
    let capacity = capacity.parse::<usize>()?;
    ensure!(
        count < capacity,
        "usable Realm entrance is full or has zero capacity"
    );
    Ok(count)
}

impl Brain {
    pub(super) fn realm_portal(
        &mut self,
        v: &PlayerView,
        grid: &Grid,
    ) -> Result<Option<(i32, Pos)>> {
        // The Nexus hedge route can leave sight of a portal before turning back toward it.
        if let Some((Goal::Portal(id), until, position)) = self.travel
            && self.goal == Goal::Portal(id)
            && v.time_ms < until
            && position.distance(v.position) >= crate::world::PLAYER_VIEW_RADIUS
            && !self.blocked.contains_key(&tile(position))
        {
            return Ok(Some((id, position)));
        }
        let choices = v
            .objects
            .iter()
            .filter(|p| {
                p.kind == 0x712 && p.usable && !self.blocked.contains_key(&tile(p.position))
            })
            .filter(|p| {
                v.position.distance(p.position) <= 2. || grid.next(v.position, p.position).is_some()
            })
            .map(|p| {
                Ok((
                    p.id,
                    population(&p.name)
                        .with_context(|| format!("Realm portal {} {:?}", p.id, p.name))?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        if choices.is_empty() {
            return Ok(None);
        }
        let id = match self.goal {
            Goal::Portal(id) if choices.iter().any(|(candidate, _)| *candidate == id) => id,
            _ => choose(v.session.account, &choices, &mut self.rng)?,
        };
        let portal = v
            .objects
            .iter()
            .find(|p| p.id == id)
            .context("selected Realm portal missing")?;
        Ok(Some((id, portal.position)))
    }
}
