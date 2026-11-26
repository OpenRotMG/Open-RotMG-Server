use super::*;

use super::BehaviorKind as K;
use super::Field as F;
pub(super) fn authored(e: &Element) -> Result<RowOrigin> {
    ensure!(
        e.required("provenance")? == "authored",
        "expected authored provenance"
    );
    Ok(RowOrigin {
        provenance: Provenance::Authored,
        origin: e.required("origin")?.into(),
        reason: e.required("reason")?.into(),
    })
}

pub(super) fn compile(e: &Element, link: &Link<'_>) -> Result<RealmRules> {
    e.attributes(&[])?;
    ensure!(e.text.is_empty(), "unexpected RealmRules text");
    let mut density = None;
    let mut camps = BTreeMap::new();
    let mut reproduction = BTreeMap::<_, Vec<_>>::new();
    let mut sites = Vec::new();
    for c in &e.children {
        (|| {
            let evidence = authored(c)?;
            match c.tag.as_str() {
                "Density" => {
                    c.qualified_attributes(&["divisors", "numerator", "denominator"])?;
                    c.leaf()?;
                    let divisors: [u32; 12] = c
                        .required("divisors")?
                        .split(',')
                        .map(integer)
                        .collect::<Result<Vec<_>>>()?
                        .try_into()
                        .map_err(|_| anyhow::anyhow!("expected 12 terrain divisors"))?;
                    let numerator = integer(c.required("numerator")?)?;
                    let denominator = integer(c.required("denominator")?)?;
                    ensure!(
                        divisors
                            .iter()
                            .all(|d| *d > 0 && d.checked_mul(denominator).is_some())
                            && (1..=100).contains(&numerator)
                            && numerator <= denominator,
                        "invalid Realm density ratio/divisor"
                    );
                    ensure!(
                        density
                            .replace(Density {
                                divisors,
                                numerator,
                                denominator,
                                evidence
                            })
                            .is_none(),
                        "duplicate Density"
                    );
                }
                "Camp" => {
                    c.qualified_attributes(&["root"])?;
                    ensure!(c.text.is_empty(), "unexpected Camp text");
                    let leader = link.object(c.required("root")?)?;
                    let mut roles = Vec::new();
                    for r in &c.children {
                        (|| {
                            ensure!(r.tag == "Member", "expected Camp Member");
                            r.attributes(&["object", "initial", "max", "interval"])?;
                            r.leaf()?;
                            let kind = link.object(r.required("object")?)?;
                            let initial = integer(r.required("initial")?)? as usize;
                            let maximum = integer(r.required("max")?)? as usize;
                            let interval = r.optional("interval", millis)?;
                            ensure!(
                                maximum <= 31
                                    && initial <= maximum
                                    && interval
                                        .map_or(initial == maximum && initial > 0, |ms| ms > 0
                                            && maximum > 0),
                                "invalid Camp Member bounds/interval"
                            );
                            ensure!(
                                !roles.iter().any(|r: &CampRole| r.kind == kind),
                                "duplicate Camp Member {kind:#x}"
                            );
                            roles.push(CampRole {
                                kind,
                                initial,
                                maximum,
                                interval,
                            });
                            Ok(())
                        })()
                        .with_context(|| r.context())?;
                    }
                    let camp = CampRule {
                        leader,
                        roles,
                        evidence,
                    };
                    ensure!(
                        (2..=32).contains(&camp.size()),
                        "invalid Camp placement capacity"
                    );
                    ensure!(
                        camps.insert(leader, Arc::new(camp)).is_none(),
                        "duplicate Camp"
                    );
                }
                "Reproduce" => {
                    c.qualified_attributes(&["parent", "child", "radius", "max", "cooldown"])?;
                    c.leaf()?;
                    let parent = link.object(c.required("parent")?)?;
                    let child = link.object(c.required("child")?)?;
                    let radius = number(c.required("radius")?)? as f32;
                    let maximum = integer(c.required("max")?)? as usize;
                    let cooldown = millis(c.required("cooldown")?)?;
                    ensure!(
                        radius > 0.
                            && radius <= 100.
                            && (1..=100).contains(&maximum)
                            && cooldown > 0,
                        "invalid Reproduce density/clock"
                    );
                    let rules = reproduction.entry(parent).or_default();
                    ensure!(
                        !rules.iter().any(|r: &ReproduceRule| r.child == child),
                        "duplicate Reproduce child"
                    );
                    rules.push(ReproduceRule {
                        child,
                        radius,
                        maximum,
                        cooldown,
                        evidence,
                    });
                }
                "NamedSite" => {
                    c.qualified_attributes(&[
                        "defender",
                        "terrains",
                        "min",
                        "max",
                        "anchorsReplaceRoll",
                        "role",
                        "phase",
                        "chance",
                        "roundRobin",
                        "anchor",
                    ])?;
                    ensure!(c.text.is_empty(), "unexpected NamedSite text");
                    let kind = link.object(c.required("defender")?)?;
                    let terrains = c
                        .required("terrains")?
                        .split(',')
                        .map(terrain)
                        .collect::<Result<Vec<_>>>()?;
                    let min = integer(c.required("min")?)? as usize;
                    let max = integer(c.required("max")?)? as usize;
                    let anchors_replace_roll = c.required("anchorsReplaceRoll")?.parse::<bool>()?;
                    ensure!(
                        min <= max
                            && max <= 100
                            && !sites.iter().any(|r: &NamedSite| r.kind == kind),
                        "invalid or duplicate NamedSite"
                    );
                    let role = match c.required("role")? {
                        "finite" => SiteRole::Finite,
                        "quest" => SiteRole::Quest,
                        "closure" => SiteRole::Closure,
                        "hero" => SiteRole::Hero,
                        "event" => SiteRole::Event,
                        value => bail!("unknown NamedSite role {value}"),
                    };
                    let phase = match c.attr("phase") {
                        None => SitePhase::Landmarks,
                        Some("after-population") => SitePhase::AfterPopulation,
                        Some(value) => bail!("unknown NamedSite phase {value}"),
                    };
                    let chance = c
                        .attrs
                        .get("chance")
                        .map(|s| -> Result<_> {
                            let (n, d) = s
                                .split_once('/')
                                .context("expected NamedSite chance numerator/denominator")?;
                            let (n, d) = (integer(n)?, integer(d)?);
                            ensure!(n > 0 && n <= d, "invalid NamedSite chance");
                            Ok((n, d))
                        })
                        .transpose()?;
                    let round_robin = c
                        .optional("roundRobin", str::parse::<bool>)?
                        .unwrap_or(false);
                    let anchor = c.optional("anchor", |s| link.object(s))?;
                    ensure!(
                        terrains.iter().copied().collect::<BTreeSet<_>>().len() == terrains.len(),
                        "duplicate NamedSite terrain"
                    );
                    ensure!(
                        !round_robin || chance.is_none() && min == max && role == SiteRole::Event,
                        "roundRobin requires fixed-count event sites"
                    );
                    ensure!(
                        anchor.is_none() || role == SiteRole::Event,
                        "site anchor requires event role"
                    );
                    let mut island = None;
                    let mut interactive_tiles = BTreeMap::new();
                    for child in &c.children {
                        match child.tag.as_str() {
                            "InteractiveTile" => {
                                child.attributes(&["object", "mark"])?;
                                child.leaf()?;
                                let mark = u8::try_from(integer(child.required("mark")?)?)?;
                                ensure!(
                                    mark != 0
                                        && interactive_tiles
                                            .insert(mark, link.object(child.required("object")?)?)
                                            .is_none(),
                                    "invalid or duplicate InteractiveTile mark {mark}"
                                );
                            }
                            "Island" => {
                                ensure!(island.is_none(), "duplicate NamedSite Island");
                                let node = child;
                                ensure!(
                                    role == SiteRole::Event
                                        && anchor.is_none()
                                        && !anchors_replace_roll,
                                    "Island requires an unanchored event site"
                                );
                                node.attributes(&[])?;
                                ensure!(node.text.is_empty(), "unexpected Island text");
                                let mut offsets = BTreeSet::new();
                                let mut placements = Vec::new();
                                for place in &node.children {
                                    (|| {
                                        ensure!(place.tag == "Place", "expected Island Place");
                                        place.attributes(&["object", "x", "y", "ownership"])?;
                                        place.leaf()?;
                                        let (x, y) = (
                                            i32::try_from(integer(place.required("x")?)?)?,
                                            i32::try_from(integer(place.required("y")?)?)?,
                                        );
                                        ensure!(
                                            (0..32).contains(&x)
                                                && (0..29).contains(&y)
                                                && (x, y) != (16, 13)
                                                && offsets.insert((x, y)),
                                            "occupied or out-of-bounds Island offset {x},{y}"
                                        );
                                        let ownership = match place.required("ownership")? {
                                            "defender" => IslandOwnership::Defender,
                                            "independent-scenery" => {
                                                IslandOwnership::IndependentScenery
                                            }
                                            other => {
                                                bail!("unknown Island placement ownership {other}")
                                            }
                                        };
                                        placements.push(IslandPlacement {
                                            kind: link.object(place.required("object")?)?,
                                            x,
                                            y,
                                            ownership,
                                        });
                                        Ok(())
                                    })()
                                    .with_context(|| place.context())?;
                                }
                                island = Some(placements);
                            }
                            _ => bail!("unknown NamedSite child {}", child.tag),
                        }
                    }
                    sites.push(NamedSite {
                        kind,
                        terrains,
                        min,
                        max,
                        anchors_replace_roll,
                        role,
                        phase,
                        chance,
                        round_robin,
                        anchor,
                        island,
                        interactive_tiles,
                        evidence,
                    });
                }
                _ => bail!("unknown RealmRules child {}", c.tag),
            }
            Ok(())
        })()
        .with_context(|| c.context())?;
    }
    Ok(RealmRules {
        density: density.context("missing RealmRules Density")?,
        camps,
        reproduction,
        sites,
        source: e.source.clone(),
    })
}

pub(super) fn validate(game: &GameData, realm: &RealmRules) -> Result<()> {
    let enemy = |kind, context: &str| {
        game.admission(kind, AdmissionRole::Scripted, context)?;
        let p = &game.properties[&kind];
        ensure!(
            p.enemy && p.hp > 0,
            "{context}: {kind:#x} is not a live enemy"
        );
        Ok(())
    };
    let habitat = |kind| {
        game.definitions
            .get(&kind)
            .and_then(|d| d.population.terrain)
    };
    for camp in realm.camps.values() {
        let context = format!("Camp {:#x}: {}", camp.leader, camp.evidence.origin);
        enemy(camp.leader, &context)?;
        let terrain =
            habitat(camp.leader).with_context(|| format!("{context}: missing linked Terrain"))?;
        for role in &camp.roles {
            enemy(role.kind, &context)?;
            ensure!(
                habitat(role.kind).is_none_or(|t| t == terrain),
                "{context}: Member {:#x} contradicts linked Terrain",
                role.kind
            );
        }
        // Independent placement and an actor-owned ongoing birth cannot claim the same slot.
        for parent in std::iter::once(camp.leader).chain(camp.roles.iter().map(|r| r.kind)) {
            for b in game.definitions[&parent]
                .states
                .iter()
                .flat_map(|s| &s.behaviors)
            {
                if matches!(
                    b.kind,
                    K::MakeMinions | K::MaintainProtectors | K::ObjectToss
                ) {
                    for field in [F::ChildId, F::ChildIds, F::GroupId] {
                        if let Some(Value::Objects(ids)) = b.fields.get(&field) {
                            ensure!(
                                camp.roles.iter().all(|r| !ids.contains(&r.kind)),
                                "{context}: conflicts with linked birth at {}:{} ({})",
                                b.source.file,
                                b.source.line,
                                b.source.object
                            );
                        }
                    }
                }
            }
        }
    }
    for (parent, rules) in &realm.reproduction {
        for r in rules {
            let context = format!(
                "Reproduce {parent:#x}->{:#x}: {}",
                r.child, r.evidence.origin
            );
            enemy(*parent, &context)?;
            enemy(r.child, &context)?;
            ensure!(
                habitat(*parent).is_some() && habitat(*parent) == habitat(r.child),
                "{context}: crosses linked Realm habitats"
            );
        }
    }
    let mut ownership = BTreeSet::new();
    for site in &realm.sites {
        let context = format!("NamedSite {:#x}: {}", site.kind, site.evidence.origin);
        enemy(site.kind, &context)?;
        for kind in std::iter::once(site.kind).chain(site.anchor) {
            game.admission(kind, AdmissionRole::Scripted, &context)?;
            let p = &game.definitions[&kind].population;
            ensure!(
                ownership.insert(kind)
                    && !(p.terrain.is_some()
                        && p.weight.is_some_and(|w| w > 0.)
                        && p.per_world_max.is_none())
                    && realm
                        .camps
                        .values()
                        .all(|c| c.leader != kind && c.roles.iter().all(|r| r.kind != kind))
                    && realm
                        .reproduction
                        .values()
                        .flatten()
                        .all(|r| r.child != kind),
                "{context}: duplicate Realm placement ownership for {kind:#x}"
            );
        }
        for place in site.island.iter().flatten() {
            match place.ownership {
                IslandOwnership::Defender => enemy(place.kind, &context)?,
                IslandOwnership::IndependentScenery => {
                    game.entity_admission(
                        place.kind,
                        AdmissionRole::Native(NativeService::Scenery),
                        &context,
                    )?;
                }
            }
        }
        if let Some(anchor) = site.anchor {
            let owns_root = game.definitions[&anchor].states.iter().flat_map(|s| &s.behaviors)
                .any(|b| b.kind == K::MakeMinions && matches!(b.fields.get(&F::ChildIds), Some(Value::Objects(ids)) if ids.as_slice() == [site.kind]));
            ensure!(
                owns_root,
                "{context}: reserved anchor {anchor:#x} does not own the selected defender"
            );
        }
    }
    Ok(())
}
