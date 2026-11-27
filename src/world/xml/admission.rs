use super::*;
use crate::content::game::{AdmissionRole, Definition, GameData, NativeService};

pub(super) fn definition<'a>(
    game: &'a GameData,
    kind: u16,
    context: &str,
) -> Result<&'a Definition> {
    let declared = game.definitions.get(&kind).with_context(|| {
        format!("{context}: kind={kind:#x} missing explicit admission definition")
    })?;
    game.admission(kind, declared.admission_role()?, context)
}

pub(super) fn raw<'a>(game: &'a GameData, kind: u16, context: &str) -> Result<&'a Definition> {
    let d = definition(game, kind, context)?;
    ensure!(
        !game.properties[&kind].is_portal(),
        "{context}: kind={kind:#x} physical portal requires its actual native service factory/owner scope (Directory/item)"
    );
    ensure!(
        matches!(
            d.admission_role()?,
            AdmissionRole::Scripted | AdmissionRole::Native(NativeService::Scenery)
        ),
        "{context}: kind={kind:#x} requires its actual native service factory/owner scope"
    );
    Ok(d)
}

pub(super) fn stay_above_applies(domain: Option<WorldDomain>, b: &Behavior) -> Result<bool> {
    match domain {
        Some(WorldDomain::Realm) => Ok(true),
        Some(WorldDomain::NonRealm) => Ok(false),
        None => bail!(
            "{} StayAbove requires explicit native world domain",
            b.source
        ),
    }
}
pub(in crate::world) fn terminal_only(d: &Definition) -> bool {
    matches!(d.disposition, Disposition::Active | Disposition::Passive)
        && d.states.len() == 1
        && d.states[0].id == d.root
        && d.states[0].children.is_empty()
        && d.states[0].orders.is_empty()
        && d.states[0].transitions.is_empty()
        && d.states[0].behaviors.iter().all(|b| {
            matches!(
                b.kind,
                K::MetamorphoseAtDeath | K::ReleaseSpawnAtDeath | K::VictoryAtDeath
            )
        })
}
fn static_map_geometry(d: &Definition, property: &crate::content::Object) -> Result<bool> {
    if !property.static_object {
        return Ok(false);
    }
    match d.admission_role()? {
        AdmissionRole::Native(NativeService::Scenery) => Ok(true),
        AdmissionRole::Scripted
            if d.disposition == Disposition::Passive
                && matches!(
                    property.class.as_str(),
                    "Wall" | "ConnectedWall" | "CaveWall"
                ) =>
        {
            behavior::validate(d)?;
            Ok(d.states
                .iter()
                .all(|s| s.behaviors.is_empty() && s.orders.is_empty() && s.transitions.is_empty()))
        }
        _ => Ok(false),
    }
}
fn elevation(
    definition: &Definition,
    map: &Map,
    pos: Pos,
    context: &str,
    domain: Option<WorldDomain>,
) -> Result<()> {
    for b in definition
        .states
        .iter()
        .flat_map(|s| &s.behaviors)
        .filter(|b| {
            b.fields.contains_key(&F::MinAltitude) || b.fields.contains_key(&F::MaxAltitude)
        })
    {
        if b.kind == K::StayAbove && !stay_above_applies(domain, b)? {
            continue;
        }
        ensure!(
            map.elevation_at(pos.x, pos.y).is_some(),
            "{context}: {} cell={},{} missing source elevation capability",
            b.source,
            pos.x,
            pos.y
        );
    }
    Ok(())
}

pub(in crate::world) struct MapSettings {
    pub name: Option<String>,
    pub size: Option<i32>,
    pub condition: i32,
    pub stats: Vec<(u8, i32)>,
}
impl World {
    pub(in crate::world) fn xml_map_settings(
        content: &Content,
        kind: u16,
        config: &str,
    ) -> Result<MapSettings> {
        let mut settings = MapSettings {
            name: None,
            size: None,
            condition: 0,
            stats: vec![],
        };
        for setting in config.split(';').filter(|s| !s.is_empty()) {
            let (key, value) = setting.split_once(':').context("malformed map setting")?;
            match key {
                "name" => settings.name = Some(value.into()),
                "size" => settings.size = Some(value.parse()?),
                "eff" | "conn" | "mtype" | "mcount" | "mtime" | "nstar" => {
                    let value = if let Some(hex) = value.strip_prefix("0x") {
                        u32::from_str_radix(hex, 16)? as i32
                    } else {
                        value.parse()?
                    };
                    let stat = match key {
                        "eff" => {
                            settings.condition |= value;
                            continue;
                        }
                        "conn" => 41,
                        "mtype" => 34,
                        "mcount" => 42,
                        "mtime" => 43,
                        "nstar" => 55,
                        _ => unreachable!(),
                    };
                    settings.stats.push((stat, value));
                }
                _ => bail!("unsupported map setting {key}"),
            }
        }
        if content.get(kind)?.class == "Merchant" {
            shop::stock(content, &settings.stats)?;
        }
        Ok(settings)
    }
    pub(in crate::world) fn xml_map_admission(
        content: &Content,
        map: &Map,
        domain: Option<WorldDomain>,
    ) -> Result<()> {
        let game = content
            .game
            .as_ref()
            .context("runtime requires retained validated XML GameData")?;
        ensure!(
            map.width > 0
                && map.height > 0
                && map.width <= i32::from(i16::MAX)
                && map.height <= i32::from(i16::MAX)
                && map.cells.len()
                    == (map.width as usize)
                        .checked_mul(map.height as usize)
                        .context("map dimensions overflow")?,
            "invalid native map dimensions"
        );
        ensure!(
            map.cells
                .iter()
                .filter(|cell| cell.object.is_some())
                .count()
                <= PHYSICAL_LIMIT,
            "initial physical entity bound exceeded before IDs"
        );
        let mut actors = 0;
        for (i, cell) in map.cells.iter().enumerate() {
            let Some(kind) = cell.object else { continue };
            let context = format!("initial map cell={i} kind={kind:#x}");
            let d = definition(game, kind, &context)?;
            let role = d.admission_role()?;
            game.entity_admission(kind, role, &context)?;
            ensure!(
                role != AdmissionRole::Native(NativeService::Companion),
                "{context}: companion requires actual owner/item reservation, not map placement"
            );
            let property = content.get(kind)?;
            if (d.disposition == Disposition::Active
                || (!property.static_object && !property.passive_destructible()))
                && !terminal_only(d)
            {
                actors += 1;
                ensure!(
                    actors <= ACTOR_LIMIT,
                    "{context}: initial map actor capacity exceeded before IDs: live={actors} limit={ACTOR_LIMIT}"
                );
            }
            let pos = Pos {
                x: (i % map.width as usize) as f32 + 0.5,
                y: (i / map.width as usize) as f32 + 0.5,
            };
            super::maps::preflight_program(game, d)?;
            elevation(d, map, pos, &context, domain)?;
            ensure!(
                !content.void_grounds.contains(&cell.ground) || static_map_geometry(d, property)?,
                "{context}: object on void ground"
            );
            Self::xml_map_settings(content, kind, &cell.config).with_context(|| context.clone())?;
        }
        Ok(())
    }
    pub(in crate::world) fn xml_declared_admission(
        &self,
        kind: u16,
        context: &str,
    ) -> Result<&Definition> {
        self.xml_check_catalog()?;
        definition(
            self.xml_game(),
            kind,
            &format!("world={} {context}", self.name),
        )
    }
    pub(in crate::world) fn xml_raw_admission(&self, kind: u16) -> Result<()> {
        self.xml_check_catalog()?;
        raw(
            self.xml_game(),
            kind,
            &format!("world={} raw scripted/scenery factory", self.name),
        )?;
        Ok(())
    }
    pub(in crate::world) fn xml_preflight_kind(&self, kind: u16) -> Result<()> {
        self.xml_check_catalog()?;
        let game = self.xml_game();
        let d = raw(
            game,
            kind,
            &format!("world={} raw scripted/scenery factory", self.name),
        )?;
        super::maps::preflight_program(game, d)
    }
    pub(in crate::world) fn xml_preflight_admission(&self, kind: u16, pos: Pos) -> Result<()> {
        self.xml_preflight_kind(kind)?;
        self.xml_preflight_position(kind, pos)
    }
    pub(in crate::world) fn xml_preflight_position(&self, kind: u16, pos: Pos) -> Result<()> {
        ensure!(
            self.entities.len() + self.xml.births.len() + self.pending_deployments()
                < PHYSICAL_LIMIT,
            "physical entity bound exceeded before IDs"
        );

        ensure!(
            pos.x.is_finite() && pos.y.is_finite() && self.map.cell(pos.x, pos.y).is_some(),
            "world={} kind={kind:#x} entity destination outside map",
            self.name
        );
        elevation(
            self.xml_declared_admission(kind, "entity position")?,
            &self.map,
            pos,
            &self.name,
            self.xml.domain,
        )
    }
    pub(in crate::world) fn xml_portal_definition(
        &self,
        kind: u16,
        context: &str,
    ) -> Result<&Definition> {
        ensure!(
            Arc::ptr_eq(
                &self.xml.game,
                self.content
                    .game
                    .as_ref()
                    .context("portal GameData absent")?
            ),
            "{context}: portal owning catalog changed"
        );
        let d = self.xml_declared_admission(kind, context)?;
        ensure!(
            self.xml_game().properties[&kind].is_portal(),
            "{context}: kind={kind:#x} is not a selected physical portal"
        );
        ensure!(
            matches!(
                d.admission_role()?,
                AdmissionRole::Scripted | AdmissionRole::Native(NativeService::Portal)
            ),
            "{context}: kind={kind:#x} unsupported physical portal admission role"
        );
        Ok(d)
    }
    pub(in crate::world) fn xml_preflight_portal(
        &self,
        kind: u16,
        pos: Pos,
        context: &str,
    ) -> Result<()> {
        self.xml_portal_definition(kind, context)?;
        self.xml_preflight_portal_program(kind)?;
        self.xml_preflight_position(kind, pos)?;
        ensure!(
            self.xml_live_count() < ACTOR_LIMIT,
            "{context}: portal capacity exhausted: live={} limit={ACTOR_LIMIT}",
            self.xml_live_count()
        );
        ensure!(
            self.xml_placeable(kind, pos, &[], None)?,
            "{context}: portal placement blocked"
        );
        Ok(())
    }
    pub(in crate::world) fn insert_portal_entity(
        &mut self,
        kind: u16,
        pos: Pos,
        context: &str,
    ) -> Result<i32> {
        self.xml_preflight_portal(kind, pos, context)?;
        let id = allocate_id(&self.ids)?;
        self.insert_entity_data(id, kind, pos, vec![])
    }
    pub(in crate::world) fn xml_preflight_native_entity(
        &self,
        kind: u16,
        service: NativeService,
        pos: Pos,
        context: &str,
    ) -> Result<()> {
        self.xml_check_catalog()?;
        self.xml_game()
            .admission(kind, AdmissionRole::Native(service), context)?;
        ensure!(
            service.is_world_entity() && service != NativeService::Companion,
            "{context}: scoped deployment/property descriptor is not ordinary native ingress"
        );
        self.xml_preflight_position(kind, pos)?;
        ensure!(
            self.xml_live_count() < ACTOR_LIMIT,
            "{context}: native entity capacity exhausted: live={} limit={ACTOR_LIMIT}",
            self.xml_live_count()
        );
        ensure!(
            self.xml_placeable(kind, pos, &[], None)?,
            "{context}: native placement blocked"
        );
        Ok(())
    }
    pub(in crate::world) fn insert_native_entity(
        &mut self,
        kind: u16,
        service: NativeService,
        pos: Pos,
        items: Vec<i32>,
        context: &str,
    ) -> Result<i32> {
        self.xml_preflight_native_entity(kind, service, pos, context)?;
        let id = allocate_id(&self.ids)?;
        self.insert_entity_data(id, kind, pos, items)
    }
}
