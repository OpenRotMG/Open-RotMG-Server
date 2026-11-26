mod realm;
use super::super::terrain;
use super::*;
use anyhow::{Context, bail, ensure};
use roxmltree::Node;
use sha2::{Digest, Sha256};
use std::path::{Component, PathBuf};

#[derive(Clone, Debug)]
pub(super) struct Element {
    pub(super) tag: String,
    pub(super) attrs: BTreeMap<String, String>,
    pub(super) text: String,
    pub(super) children: Vec<Element>,
    pub(super) source: Source,
}
impl Element {
    fn parse(n: Node<'_, '_>, file: &str, lines: &[usize], depth: usize) -> Result<Self> {
        ensure!(depth <= 128, "{file}: XML nesting exceeds 128");
        Ok(Self {
            tag: n.tag_name().name().into(),
            attrs: n
                .attributes()
                .map(|a| (a.name().into(), a.value().into()))
                .collect(),
            text: n
                .children()
                .filter(|c| c.is_text())
                .filter_map(|c| c.text())
                .collect::<String>()
                .trim()
                .into(),
            children: n
                .children()
                .filter(|c| c.is_element())
                .map(|c| Self::parse(c, file, lines, depth + 1))
                .collect::<Result<_>>()?,
            source: Source {
                file: file.into(),
                line: lines.partition_point(|offset| *offset <= n.range().start) as u32,
                offset: n.range().start,
                object: String::new(),
                path: String::new(),
                embeds: vec![],
            },
        })
    }
    pub(super) fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.get(name).map(String::as_str)
    }
    pub(super) fn optional<T, E>(
        &self,
        name: &str,
        parse: impl FnOnce(&str) -> std::result::Result<T, E>,
    ) -> std::result::Result<Option<T>, E> {
        self.attr(name).map(parse).transpose()
    }
    pub(super) fn required(&self, name: &str) -> Result<&str> {
        self.attr(name)
            .filter(|v| !v.is_empty())
            .with_context(|| format!("missing {name}"))
    }
    pub(super) fn attributes(&self, names: &[&str]) -> Result<()> {
        self.check_attributes(names, &[])
    }
    pub(super) fn qualified_attributes(&self, names: &[&str]) -> Result<()> {
        self.check_attributes(names, &["provenance", "origin", "reason"])
    }
    fn check_attributes(&self, names: &[&str], extra: &[&str]) -> Result<()> {
        for name in self.attrs.keys() {
            ensure!(
                names.contains(&name.as_str()) || extra.contains(&name.as_str()),
                "unknown {} attribute {name}",
                self.tag
            );
        }
        Ok(())
    }
    pub(super) fn leaf(&self) -> Result<()> {
        ensure!(
            self.children.is_empty(),
            "{} cannot have child elements",
            self.tag
        );
        Ok(())
    }
    pub(super) fn qualification(&self, label: &str) -> Result<RowOrigin> {
        let provenance = match self.required("provenance")? {
            "recovered" => Provenance::Recovered,
            "authored" => Provenance::Authored,
            "corrected" => Provenance::Corrected,
            value => bail!("unknown {label}provenance {value}"),
        };
        Ok(RowOrigin {
            provenance,
            origin: self.required("origin")?.into(),
            reason: self.required("reason")?.into(),
        })
    }
    pub(super) fn context(&self) -> String {
        self.source.to_string()
    }
}
pub(super) fn parse(file: &str, bytes: &[u8]) -> Result<Element> {
    let text = std::str::from_utf8(bytes).with_context(|| format!("{file}: invalid UTF-8"))?;
    let options = roxmltree::ParsingOptions {
        nodes_limit: 500_000,
        ..Default::default()
    };
    let doc = roxmltree::Document::parse_with_options(text, options)
        .with_context(|| format!("{file}: XML parse"))?;
    let lines: Vec<_> = std::iter::once(0)
        .chain(
            text.bytes()
                .enumerate()
                .filter(|(_, b)| *b == b'\n')
                .map(|(i, _)| i + 1),
        )
        .collect();
    Element::parse(doc.root_element(), file, &lines, 0)
}
pub(super) fn safe_path(path: &str) -> Result<PathBuf> {
    let p = Path::new(path);
    ensure!(
        !path.is_empty() && p.components().all(|c| matches!(c, Component::Normal(_))),
        "invalid relative input path {path}"
    );
    Ok(p.to_owned())
}
fn read(root: &Path, path: &str) -> Result<Vec<u8>> {
    let path = root.join(safe_path(path)?);
    let canonical = path
        .canonicalize()
        .with_context(|| format!("required input {}", path.display()))?;
    ensure!(
        canonical.starts_with(
            root.canonicalize()
                .with_context(|| format!("data root {}", root.display()))?
        ),
        "input escapes data directory: {}",
        path.display()
    );
    std::fs::read(&path).with_context(|| format!("read {}", path.display()))
}
const MANIFEST: &str = "game/123.5.1/game.xml";
pub(super) fn load(root: &Path, content: &Content) -> Result<GameData> {
    let mut inputs = BTreeMap::new();
    inputs.insert(MANIFEST.into(), read(root, MANIFEST)?);
    let manifest = parse(MANIFEST, &inputs[MANIFEST])?;
    for e in &manifest.children {
        let path = match e.tag.as_str() {
            "Include" | "Geometry" => Some(include_path(e.required("file")?)?),
            "Map" | "Evidence" => Some(e.required("file")?.to_owned()),
            _ => None,
        };
        if let Some(path) = path {
            inputs.insert(
                path.clone(),
                read(root, &path).with_context(|| e.context())?,
            );
        }
    }
    for e in &manifest.children {
        if e.tag == "Geometry" {
            let file = include_path(e.required("file")?)?;
            let doc = parse(&file, &inputs[&file])?;
            for map in doc.children.iter().filter(|e| e.tag == "Map") {
                let file = map.required("file")?;
                inputs.insert(
                    file.into(),
                    read(root, file).with_context(|| map.context())?,
                );
            }
        }
    }
    for file in ["objects.xml", "grounds.xml"] {
        inputs.insert(file.into(), read(root, file)?);
    }
    compile_at(content, inputs, Some(root))
}
fn include_path(file: &str) -> Result<String> {
    safe_path(file)?;
    ensure!(file.ends_with(".xml"), "Include must select XML");
    Ok(format!("game/123.5.1/{file}"))
}
pub(super) fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub(super) fn number(s: &str) -> Result<f64> {
    let n: f64 = s
        .parse()
        .with_context(|| format!("malformed number {s:?}"))?;
    ensure!(n.is_finite(), "nonfinite number {s}");
    Ok(n)
}
fn nonnegative(s: &str) -> Result<f64> {
    let n = number(s)?;
    ensure!(n >= 0., "negative value {s}");
    Ok(n)
}
pub(super) fn probability(s: &str) -> Result<f64> {
    let n = number(s)?;
    ensure!((0.0..=1.0).contains(&n), "probability outside [0,1]: {s}");
    Ok(n)
}
pub(super) fn integer(s: &str) -> Result<u32> {
    s.parse()
        .with_context(|| format!("invalid nonnegative integer {s:?}"))
}
macro_rules! executor_fields {
    ($($kind:ident: [$($read:ident),*] {$($field:ident = $value:literal),*};)+) => {
        pub(super) fn executor_defaults(kind: BehaviorKind) -> &'static [(Field, &'static str)] {
            match kind { $(BehaviorKind::$kind => &[$((Field::$field, $value)),*],)+ _ => &[] }
        }
        pub(crate) fn executor_reads(kind: BehaviorKind) -> &'static [Field] {
            match kind { $(BehaviorKind::$kind => &[$(Field::$read),*],)+ _ => &[] }
        }
    };
}
// The compiler materializes defaults and checks required reads before a world can run the rule.
// See docs/xml-migration-differences.md for divergences from DECA's documented defaults.
executor_fields! {
    BackAndForth: [Speed, Angle, Radius, TurnRate] {Speed = "1", Angle = "0", TurnRate = "0", Radius = "4"};
    Buzz: [AcquireRange, Speed, Acceleration, TurnRate] {
        Speed = "15", Acceleration = "5", TurnRate = "3.141592653589793", AcquireRange = "20", Cooldown = "2"
    };
    ChangeSize: [Rate, MinSize, MaxSize] {Rate = "0.2", MinSize = "60", MaxSize = "200"};
    Charge: [Range, Speed] {Range = "20", Speed = "3", Cooldown = "3"};
    Circle: [AcquireRange, Distance, Speed] {AcquireRange = "15", Distance = "8", Speed = "1"};
    ClearConditionEffectAura: [Cooldown, Range, IgnoreInvisible] {
        Cooldown = "1", Range = "0", IgnoreInvisible = "true"
    };
    ConditionEffect: [Duration] {Duration = "0"};
    ConditionEffectAura: [Cooldown, Range] {Cooldown = "1", Range = "0"};
    Decay: [Time, DropLoot, GiveXP] {Time = "60", DropLoot = "false", GiveXP = "false"};
    DeleteObject: [Radius] {Radius = "1"};
    Explode: [Range, NumShots, ArcDegrees, DropLoot, GiveXP] {
        Range = "5", NumShots = "1", ArcDegrees = "360", DropLoot = "false", GiveXP = "false"
    };
    Follow: [AcquireRange, Range, Speed, Duration, Predictive] {
        AcquireRange = "10", Range = "3", Speed = "1", Duration = "30", Cooldown = "5", Predictive = "false"
    };
    FollowOrders: [AcquireRange, Cooldown] {AcquireRange = "15", Cooldown = "3"};
    GrenadeToss: [Range, MinRange, DefaultAngle, Radius, Damage, Duration] {
        Type = "targeted", Duration = "0", Range = "8", MinRange = "0", Radius = "3", Damage = "0",
        DefaultAngle = "0"
    };
    Heal: [Range, HideVisual] {Cooldown = "10", Range = "8", HideVisual = "false"};
    HealZap: [Range, Healing] {Cooldown = "1", Range = "5", Healing = "25"};
    KeepDistance: [AcquireRange, Distance, Speed] {AcquireRange = "15", Distance = "10", Speed = "1"};
    MaintainProtectors: [NumChildren, Distance, Cooldown] {Cooldown = "30", Distance = "0"};
    MakeMinions: [MaxChildren, StartWithMinions] {StartWithMinions = "true", Cooldown = "30"};
    Metamorphose: [Time, DropLoot, GiveXP] {DropLoot = "false", GiveXP = "false"};
    MetamorphoseAtDeath: [PosJitter] {PosJitter = "0.1"};
    Move: [Speed, Angle, Distance] {Speed = "1", Distance = "3", Angle = "0", Cooldown = "10"};
    ObjectToss: [Range, MinRange, RangeJitter, DefaultAngle, CenterObject] {
        Type = "targeted", RangeJitter = "0", CenterObject = "false", Range = "8", MinRange = "0",
        DefaultAngle = "0"
    };
    Orbit: [AcquireRange, Speed, Radius, RadiusVariability, SpeedVariability, RadiusWobbleDistance, RadiusWobblePeriod, Clockwise] {
        AcquireRange = "15", Speed = "0.8", Radius = "5", RadiusWobbleDistance = "0",
        RadiusWobblePeriod = "15", Clockwise = "false", SpeedVariability = "0.2", RadiusVariability = "0.5"
    };
    PlaceMap: [] {MaxPlace = "1", Cooldown = "5"};
    PlaySound: [SoundId] {};
    Protect: [AcquireRange, ProtectionRange, ReprotectRange, Speed] {
        AcquireRange = "15", ProtectionRange = "3", Speed = "0.8"
    };
    ReleaseSpawnAtDeath: [Min, Range] {Min = "1", Range = "1"};
    RestoresBreath: [Speed, Radius] {Speed = "1", Radius = "1"};
    Say: [Broadcast, BubbleTime] {Broadcast = "false", Prob = "1", BubbleTime = "10"};
    SetAltTexture: [AltTextureId] {AltTextureId = "0"};
    Shadow: [Range, ReshadowRange, Speed] {Range = "3", Speed = "3", ReshadowRange = "0.5"};
    Shoot: [Range, NumShots, Predictive, DefaultAngle, Angle, BlastEffect] {
        Type = "targeted", Range = "8", NumShots = "1", ProjectileId = "0", Cooldown = "1", Offset = "0",
        CooldownJitter = "false", Predictive = "0", Angle = "9", BlastEffect = "0"
    };
    ShowEffect: [FlashPeriod, FlashRepeats] {FlashPeriod = "1", FlashRepeats = "1"};
    StayAbove: [Speed, MinAltitude] {MinAltitude = "200", Speed = "0.5"};
    StayBack: [Distance, Speed] {Distance = "10", Speed = "1"};
    StayCloseToSpawn: [Range, Speed] {Range = "7", Speed = "1", Cooldown = "3"};
    StayInTerrain: [Speed, Terrain] {Speed = "0.5"};
    Swirl: [Speed, Acceleration, TurnRate, Distance] {
        Distance = "1", Speed = "2", Acceleration = "2", TurnRate = "3.141592653589793"
    };
    Taunt: [Broadcast] {Broadcast = "false"};
    TrapTrigger: [Radius, RadiusVariability, MinDamage, MaxDamage, PosJitter] {
        Cooldown = "3", Radius = "1", RadiusVariability = "0", MinDamage = "10", MaxDamage = "10",
        PosJitter = "0"
    };
    Walk: [Speed] {Speed = "0.4"};
    Wander: [Speed, AvoidWater] {Speed = "0.4", AvoidWater = "false"};
    Zap: [Range, Damage, ConditionDuration, PoisonTotalDamage, PoisonDuration] {
        Cooldown = "1", Range = "8", Damage = "0", ConditionDuration = "1", PoisonTotalDamage = "0",
        PoisonDuration = "0"
    };
}
fn millis(s: &str) -> Result<u64> {
    let n = nonnegative(s)? * 1000.;
    ensure!(
        n < u64::MAX as f64 && n.fract().abs() < 0.000001,
        "time must fit integral milliseconds: {s}"
    );
    Ok(n.round() as u64)
}
fn names(s: &str) -> Result<Vec<&str>> {
    let v: Vec<_> = s.split(',').map(str::trim).collect();
    ensure!(v.iter().all(|s| !s.is_empty()), "empty reference in {s:?}");
    Ok(v)
}
pub(super) struct Link<'a> {
    pub(super) content: &'a Content,
    names: BTreeMap<String, u16>,
    groups: BTreeMap<String, BTreeSet<u16>>,
    pub(super) maps: BTreeMap<String, u32>,
    string_lists: BTreeMap<String, u32>,
    symbols: std::cell::RefCell<BTreeMap<String, u32>>,
}
impl<'a> Link<'a> {
    pub(super) fn new(content: &'a Content) -> Result<Self> {
        let mut selected = BTreeMap::new();
        for o in content.objects.values() {
            ensure!(
                selected.insert(o.name.clone(), o.kind).is_none(),
                "ambiguous selected object name {}",
                o.name
            );
        }
        Ok(Self {
            content,
            names: selected,
            groups: BTreeMap::new(),
            maps: BTreeMap::new(),
            string_lists: BTreeMap::new(),
            symbols: std::cell::RefCell::new(BTreeMap::new()),
        })
    }

    fn symbol(&self, text: &str) -> Result<u32> {
        ensure!(!text.is_empty(), "empty symbol");
        let mut symbols = self.symbols.borrow_mut();
        let next = symbols.len() as u32;
        Ok(*symbols.entry(text.into()).or_insert(next))
    }
    pub(super) fn object(&self, name: &str) -> Result<u16> {
        self.names
            .get(name)
            .copied()
            .with_context(|| format!("unresolved selected object {name:?}"))
    }
    pub(super) fn group(&self, name: &str) -> Result<Vec<u16>> {
        let members = self
            .groups
            .get(name)
            .filter(|members| !members.is_empty())
            .with_context(|| format!("unknown/empty admitted group {name:?}"))?;
        Ok(members.iter().copied().collect())
    }
    fn objects(&self, value: &str, groups: bool) -> Result<Vec<u16>> {
        let mut out = BTreeSet::new();
        for name in names(value)? {
            ensure!(
                !(groups && self.names.contains_key(name) && self.groups.contains_key(name)),
                "ambiguous object/group reference {name:?}"
            );
            if let Some(id) = self.names.get(name) {
                out.insert(*id);
            } else if groups {
                out.extend(
                    self.group(name)
                        .with_context(|| format!("unresolved object/group {name:?}"))?,
                );
            } else {
                bail!("unresolved selected object {name:?}");
            }
        }
        Ok(out.into_iter().collect())
    }
    fn group_names(&self, value: &str) -> Result<Vec<u16>> {
        let mut out = BTreeSet::new();
        for name in names(value)? {
            out.extend(self.group(name)?);
        }
        Ok(out.into_iter().collect())
    }
    pub(super) fn grounds(&self, value: &str) -> Result<Vec<u8>> {
        names(value)?
            .iter()
            .map(|name| {
                let ids: Vec<_> = self
                    .content
                    .ground_names
                    .iter()
                    .filter(|(_, v)| v.as_str() == *name)
                    .map(|(id, _)| *id)
                    .collect();
                ensure!(
                    ids.len() == 1,
                    "unresolved/ambiguous ground {name:?}; selected matches={ids:?}"
                );
                Ok(ids[0])
            })
            .collect()
    }
}
fn text_parts(text: &str) -> Result<Vec<TextPart>> {
    ensure!(!text.is_empty(), "empty text");
    let mut parts = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('{') {
        let (prefix, after) = rest.split_at(start);
        ensure!(!prefix.contains('}'), "unmatched text brace");
        if !prefix.is_empty() {
            parts.push(TextPart::Literal(prefix.into()));
        }
        let end = after.find('}').context("unterminated text token")?;
        parts.push(match &after[1..end] {
            "KILLER" => TextPart::Killer,
            "PLAYER" => TextPart::Player,
            "COUNT" => TextPart::Count,
            "HITPOINTS" => TextPart::Hitpoints,
            token => bail!("unknown text token {token}"),
        });
        rest = &after[end + 1..];
    }
    ensure!(!rest.contains('}'), "unmatched text brace");
    if !rest.is_empty() {
        parts.push(TextPart::Literal(rest.into()));
    }
    Ok(parts)
}
fn string_list(e: &Element) -> Result<StringList> {
    ensure!(e.tag == "StringList", "unknown StringLists child {}", e.tag);
    e.attributes(&["id"])?;
    let name = e.required("id")?.to_owned();
    let mut categories = BTreeMap::new();
    for category in &e.children {
        ensure!(
            category.tag == "Strings",
            "unknown StringList child {}",
            category.tag
        );
        category.attributes(&["id"])?;
        let mut texts = Vec::new();
        for text in &category.children {
            ensure!(text.tag == "String", "unknown Strings child {}", text.tag);
            text.attributes(&[])?;
            text.leaf()?;
            texts.push(text_parts(&text.text)?);
        }
        ensure!(!texts.is_empty(), "empty Strings category");
        ensure!(
            categories
                .insert(category.required("id")?.to_owned(), texts)
                .is_none(),
            "duplicate Strings category"
        );
    }
    ensure!(!categories.is_empty(), "empty StringList");
    Ok(StringList {
        name,
        source: e.source.clone(),
        categories,
    })
}
fn condition(name: &str) -> Result<Condition> {
    // The recovered server-only immunity must not invent a selected-client wire bit.
    if name == "Stun Immune" {
        return Ok(Condition::StunImmune);
    }
    Ok(Condition::Selected(match name {
        "Invisible" => 13,
        "Speedy" => 15,
        "Healing" => 18,
        "Damaging" => 19,
        "Berserk" => 20,
        "Stasis" => 22,
        "Stasis Immune" => 23,
        "Invincible" => 24,
        "Invulnerable" => 25,
        "Armored" => 26,
        "Hexed" => 28,
        _ => crate::condition::projectile_effect(name)?,
    }))
}
fn behavior_fields(e: &Element, kind: BehaviorKind) -> impl Iterator<Item = Result<(Field, &str)>> {
    e.attrs
        .iter()
        .filter(|(name, _)| name.as_str() != "bucket")
        .map(move |(name, text)| {
            let field = Field::parse(name)?;
            ensure!(
                kind.fields().contains(&field),
                "attribute {name} not valid for {kind:?}"
            );
            Ok((field, text.as_str()))
        })
}
fn syntax(e: &Element) -> Result<()> {
    let result = (|| {
        let attrs: &[&str] = match e.tag.as_str() {
            "Object" => {
                e.qualified_attributes(&["type", "id", "disposition", "nativeService"])?;
                &[]
            }
            "Group" | "SpawnProb" | "PerRealmMax" | "Terrain" | "XpMult" | "Spawn" | "Mean"
            | "StdDev" | "Min" | "Max" => &[],
            "State" => &["id"],
            "Order" => &["targetIds"],
            "DefineEmbed" | "Embed" | "Param" => &["name"],
            "Loot" | "SoulboundLoot" => {
                loot_syntax(e)?;
                return Ok(());
            }
            "Transition" => &[
                "afterTime",
                "playerWithin",
                "noPlayerWithin",
                "hitpointsLessThan",
                "random",
                "exist",
                "noneExist",
                "onTile",
                "hasTag",
                "noTag",
                "hasGlobalTag",
                "noGlobalTag",
                "playerSays",
            ],
            "Behavior" => {
                e.leaf()?;
                let kind = BehaviorKind::parse(&e.text)?;
                behavior_fields(e, kind).try_for_each(|field| field.map(drop))?;
                return Ok(());
            }
            _ => bail!(
                "unknown server element {} (selected-client properties may not be overlaid)",
                e.tag
            ),
        };
        if e.tag != "Object" {
            e.attributes(attrs)?;
        }
        if ["Object", "State", "Order", "DefineEmbed", "Embed"].contains(&e.tag.as_str()) {
            ensure!(e.text.is_empty(), "unexpected text in {}", e.tag);
        }
        if [
            "Group",
            "Param",
            "Transition",
            "SpawnProb",
            "PerRealmMax",
            "Terrain",
            "XpMult",
            "Mean",
            "StdDev",
            "Min",
            "Max",
        ]
        .contains(&e.tag.as_str())
        {
            e.leaf()?;
        }
        let mut definitions = BTreeSet::new();
        for c in &e.children {
            if c.tag == "DefineEmbed" {
                ensure!(
                    definitions.insert(c.required("name")?),
                    "duplicate embed definition"
                );
            }
            syntax(c)?;
        }
        Ok(())
    })();
    result.with_context(|| e.context())
}
type EmbedScope = BTreeMap<String, Element>;
fn embed_scope(e: &Element) -> Result<EmbedScope> {
    e.children
        .iter()
        .filter(|c| c.tag == "DefineEmbed")
        .map(|c| Ok((c.required("name")?.to_owned(), c.clone())))
        .collect()
}
fn resolve_embed<'a>(scopes: &'a [EmbedScope], name: &str) -> Result<(usize, &'a Element)> {
    scopes
        .iter()
        .enumerate()
        .rev()
        .find_map(|(i, s)| s.get(name).map(|d| (i, d)))
        .with_context(|| format!("unresolved embed {name}"))
}
fn check_templates(
    e: &Element,
    inherited: &[EmbedScope],
    stack: &mut Vec<(String, usize)>,
    checked: &mut BTreeSet<(String, usize)>,
) -> Result<()> {
    let mut scopes = inherited.to_vec();
    scopes.push(embed_scope(e)?);
    for child in &e.children {
        let result = (|| {
            if child.tag == "Embed" {
                let name = child.required("name")?;
                let (index, def) = resolve_embed(&scopes, name)?;
                let key = (def.source.file.clone(), def.source.offset);
                ensure!(!stack.contains(&key), "recursive embed {name}");
                if !checked.contains(&key) {
                    ensure!(stack.len() < 64, "embed dependency depth exceeds 64");
                    stack.push(key.clone());
                    check_templates(def, &scopes[..=index], stack, checked)?;
                    stack.pop();
                    checked.insert(key);
                }
            } else if child.tag == "DefineEmbed" {
                let key = (child.source.file.clone(), child.source.offset);
                if !checked.contains(&key) {
                    ensure!(
                        !stack.contains(&key) && stack.len() < 64,
                        "recursive embed dependency"
                    );
                    stack.push(key.clone());
                    check_templates(child, &scopes, stack, checked)?;
                    stack.pop();
                    checked.insert(key);
                }
            } else {
                check_templates(child, &scopes, stack, checked)?;
            }
            Ok(())
        })();
        result.with_context(|| child.context())?;
    }
    Ok(())
}
#[derive(Clone)]
struct Parameter {
    value: String,
    // Inherited clones share reads; a shadowing argument gets its own usage marker.
    used: std::rc::Rc<std::cell::Cell<bool>>,
}
fn expand(
    e: &Element,
    inherited: &[EmbedScope],
    params: &BTreeMap<String, Parameter>,
    stack: &mut Vec<(String, usize)>,
    count: &mut usize,
) -> Result<Vec<Element>> {
    let result = (|| {
        *count += 1;
        ensure!(
            *count <= 100_000 && stack.len() < 64,
            "embed expansion limit"
        );
        let mut scopes = inherited.to_vec();
        scopes.push(embed_scope(e)?);
        let substitute = |s: &str, text: bool| -> Result<String> {
            let mut out = s.to_owned();
            for (key, param) in params {
                let placeholder = format!("{{{key}}}");
                if out.contains(&placeholder) {
                    param.used.set(true);
                    out = out.replace(&placeholder, &param.value);
                }
            }
            if out.contains('{') || out.contains('}') {
                ensure!(text, "unbound parameter {s:?}");
                text_parts(&out).with_context(|| format!("unbound parameter {s:?}"))?;
            }
            Ok(out)
        };
        let mut children = Vec::new();
        for child in &e.children {
            if child.tag == "DefineEmbed" {
                continue;
            }
            if child.tag == "Embed" {
                child.attributes(&["name"])?;
                let name = child.required("name")?;
                let (scope_index, definition) = resolve_embed(&scopes, name)?;
                let mut def = definition.clone();
                let key = (def.source.file.clone(), def.source.offset);
                ensure!(!stack.contains(&key), "recursive embed {name}");
                let mut bound = params.clone();
                let mut keys = BTreeSet::new();
                for p in &child.children {
                    ensure!(p.tag == "Param", "unknown Embed child {}", p.tag);
                    p.attributes(&["name"])?;
                    p.leaf()?;
                    let key = p.required("name")?;
                    ensure!(keys.insert(key), "duplicate parameter {key}");
                    bound.insert(
                        key.into(),
                        Parameter {
                            value: substitute(&p.text, true)?,
                            used: std::rc::Rc::new(std::cell::Cell::new(false)),
                        },
                    );
                }
                fn annotate(e: &mut Element, call: &Element, name: &str) {
                    e.source.embeds.extend(call.source.embeds.clone());
                    e.source
                        .embeds
                        .push(format!("{name}@{}:{}", call.source.file, call.source.line));
                    for c in &mut e.children {
                        annotate(c, call, name);
                    }
                }
                annotate(&mut def, child, name);
                stack.push(key);
                children.extend(expand(&def, &scopes[..=scope_index], &bound, stack, count)?);
                stack.pop();
                for key in keys {
                    ensure!(
                        bound[key].used.get(),
                        "unused embed parameter {key} at {}",
                        child.context()
                    );
                }
            } else {
                let mut c = child.clone();
                c.attrs = c
                    .attrs
                    .iter()
                    .map(|(k, v)| Ok((k.clone(), substitute(v, k == "text")?)))
                    .collect::<Result<_>>()?;
                c.text = substitute(&c.text, false)?;
                c.children = expand(child, &scopes, params, stack, count)?;
                children.push(c);
            }
        }
        Ok(children)
    })();
    result.with_context(|| e.context())
}
struct Builder<'a, 'b> {
    link: &'a Link<'b>,
    string_lists: &'a [StringList],
    object: u16,
    states: Vec<State>,
    pending: Vec<(StateId, Element)>,
    deps: BTreeSet<u16>,
    local: u32,
}
impl Builder<'_, '_> {
    fn next_local(&mut self) -> u32 {
        let n = self.local;
        self.local += 1;
        n
    }
    fn state(
        &mut self,
        e: &Element,
        parent: Option<StateId>,
        name: &str,
        path: &str,
    ) -> Result<StateId> {
        let mut e = e.clone();
        e.source.object = self.link.content.objects[&self.object].name.clone();
        e.source.path = path.into();
        let result: Result<StateId> = (|| {
            let id = self.states.len() as u32;
            self.states.push(State {
                id,
                name: name.into(),
                parent,
                children: vec![],
                behaviors: vec![],
                transitions: vec![],
                orders: vec![],
                source: e.source.clone(),
            });
            let mut siblings = BTreeSet::new();
            for mut c in e.children.clone() {
                c.source.object = e.source.object.clone();
                c.source.path = path.into();
                let result: Result<()> = (|| {
                    match c.tag.as_str() {
                        "State" => {
                            c.attributes(&["id"])?;
                            let name = c.required("id")?;
                            ensure!(
                                siblings.insert(name.to_owned()),
                                "duplicate sibling state {name}"
                            );
                            let child =
                                self.state(&c, Some(id), name, &format!("{path}/{name}"))?;
                            self.states[id as usize].children.push(child);
                        }
                        "Behavior" => {
                            let b = self.behavior(&c)?;
                            self.states[id as usize].behaviors.push(b);
                        }
                        "Transition" => {
                            c.leaf()?;
                            self.pending.push((id, c.clone()));
                        }
                        "Order" => {
                            c.attributes(&["targetIds"])?;
                            let targets =
                                c.optional("targetIds", |s| self.link.objects(s, true))?;
                            if let Some(ids) = &targets {
                                self.deps.extend(ids);
                            }
                            let local = self.next_local();
                            let state =
                                self.state(&c, None, "@order", &format!("{path}/@order[{local}]"))?;
                            self.states[id as usize].orders.push(Order {
                                local,
                                targets,
                                state,
                                source: c.source.clone(),
                            });
                        }
                        _ => bail!("unknown server element {}", c.tag),
                    }
                    Ok(())
                })();
                result.with_context(|| c.context())?;
            }
            Ok(id)
        })();
        result.with_context(|| e.context())
    }
    fn behavior(&mut self, e: &Element) -> Result<Behavior> {
        e.leaf()?;
        let kind = BehaviorKind::parse(&e.text)?;
        let mut fields = BTreeMap::new();
        ensure!(e.attr("bucket") != Some(""), "empty bucket");
        for field in behavior_fields(e, kind) {
            let (field, text) = field?;
            fields.insert(field, self.value(kind, field, text)?);
        }
        use BehaviorKind as K;
        use Field as F;
        // Source aliases must precede defaults; a default is not an explicitly conflicting value.
        let alias = match kind {
            K::Buzz => Some((F::Range, F::AcquireRange, true)),
            K::Protect => Some((F::Range, F::ReprotectRange, true)),
            K::Follow => Some((F::Distance, F::AcquireRange, false)),
            K::Circle | K::KeepDistance => Some((F::Range, F::Distance, false)),
            _ => None,
        };
        if let Some((legacy, canonical, retain)) = alias
            && let Some(Value::Number(value)) = fields.get(&legacy).cloned()
        {
            ensure!(
                fields
                    .get(&canonical)
                    .is_none_or(|v| matches!(v, Value::Number(n) if *n == value)),
                "conflicting explicit {kind:?} {legacy:?}/{canonical:?} aliases"
            );
            fields.entry(canonical).or_insert(Value::Number(value));
            if !retain {
                fields.remove(&legacy);
            }
        }
        let required: &[Field] = match kind {
            K::MakeMinions if fields.contains_key(&F::GroupId) => &[F::MaxChildren],
            K::MakeMinions => &[F::ChildIds, F::MaxChildren],
            K::MaintainProtectors => &[F::ChildIds, F::NumChildren],
            K::Metamorphose | K::MetamorphoseAtDeath | K::ReleaseSpawnAtDeath | K::ObjectToss => {
                &[F::ChildId]
            }
            K::ConditionEffect | K::ConditionEffectAura | K::ClearConditionEffectAura => {
                &[F::Effect]
            }
            K::SetTag | K::ClearTag | K::SetGlobalTag | K::ClearGlobalTag => &[F::TagName],
            K::PlaceMap => &[F::Map],
            K::ChangeGround => &[F::GroundId],
            K::DeleteObject => &[F::ObjectId],
            K::CopyDamageRecord => &[F::TargetId],
            K::Explode if !matches!(fields.get(&F::NumShots), Some(Value::Integer(0))) => {
                &[F::ProjectileId]
            }
            K::PlaySound => &[F::SoundId],
            K::Say => &[F::Text],
            K::Taunt => &[F::ListId],
            _ => &[],
        };
        if let Some(Value::Mode(mode)) = fields.get(&F::Type) {
            ensure!(
                matches!(
                    (kind, mode),
                    (K::ShowEffect, Mode::Flash)
                        | (
                            K::Shoot | K::Explode | K::GrenadeToss | K::ObjectToss,
                            Mode::Auto | Mode::Targeted
                        )
                ),
                "behavior type incompatible with {:?}",
                kind
            );
        }
        for f in required {
            ensure!(
                fields.contains_key(f),
                "missing required {:?} for {:?}",
                f,
                kind
            );
        }
        let defaults = executor_defaults(kind);
        for (f, text) in defaults {
            if (kind == K::Shoot && *f == F::ProjectileId && fields.contains_key(&F::ChildId))
                || (kind == K::Explode
                    && *f == F::ArcDegrees
                    && matches!(fields.get(&F::NumShots), Some(Value::Integer(0))))
            {
                continue;
            }
            ensure!(
                kind.fields().contains(f),
                "default field {:?} is not permitted for {:?}",
                f,
                kind
            );
            if !fields.contains_key(f) {
                fields.insert(*f, self.value(kind, *f, text)?);
            }
        }
        if kind == K::Shoot && !fields.contains_key(&F::DefaultAngle) {
            let auto = matches!(fields.get(&F::Type), Some(Value::Mode(Mode::Auto)));
            let angle = self.value(kind, F::DefaultAngle, if auto { "90" } else { "0" })?;
            fields.insert(F::DefaultAngle, angle);
        }
        if kind == K::Protect {
            let outer = fields[&F::ProtectionRange].clone();
            fields.entry(F::ReprotectRange).or_insert(outer);
            ensure!(
                matches!((&fields[&F::ReprotectRange], &fields[&F::ProtectionRange]),
                (Value::Number(inner), Value::Number(outer)) if inner <= outer),
                "Protect reprotectRange exceeds final protectionRange"
            );
        }
        if kind == K::Orbit
            && matches!(fields.get(&F::RadiusWobbleDistance), Some(Value::Number(n)) if *n > 0.)
        {
            ensure!(
                !matches!(fields.get(&F::RadiusWobblePeriod), Some(Value::Millis(0))),
                "positive Orbit wobble requires positive radiusWobblePeriod"
            );
        }
        if kind == K::StayInTerrain && !fields.contains_key(&F::Terrain) {
            fields.insert(F::Terrain, Value::NativeTerrain);
        }
        for field in [F::MaxChildren, F::NumChildren] {
            if let Some(Value::Integer(n)) = fields.get(&field) {
                ensure!(*n > 0 && *n <= 4096, "invalid bounded child population");
            }
        }
        for (min, max) in [
            (F::MinSize, F::MaxSize),
            (F::Min, F::Max),
            (F::MinDamage, F::MaxDamage),
        ] {
            if let (Some(Value::Number(a)), Some(Value::Number(b))) =
                (fields.get(&min), fields.get(&max))
            {
                ensure!(a <= b, "reversed {:?}/{:?}", min, max);
            }
            if let (Some(Value::Integer(a)), Some(Value::Integer(b))) =
                (fields.get(&min), fields.get(&max))
            {
                ensure!(a <= b, "reversed {:?}/{:?}", min, max);
            }
        }
        for f in executor_reads(kind) {
            // A terminal (numShots 0) Explode fires no volley, so it never reads arcDegrees.
            let volley = !matches!(fields.get(&F::NumShots), Some(Value::Integer(0)));
            ensure!(
                fields.contains_key(f) || (kind, *f, volley) == (K::Explode, F::ArcDegrees, false),
                "{kind:?} executor reads {f:?}, but the behavior neither sets it nor has a compiled default"
            );
        }
        let emission = match kind {
            K::Shoot if fields.contains_key(&F::ChildId) => {
                ensure!(
                    !fields.contains_key(&F::ProjectileId),
                    "Shoot cannot emit both child and projectile"
                );
                let Value::Objects(ids) = &fields[&F::ChildId] else {
                    unreachable!()
                };
                ensure!(
                    ids.len() == 1,
                    "Shoot childId requires exactly one selected child"
                );
                ensure!(
                    !matches!(fields.get(&F::NumShots), Some(Value::Integer(0))),
                    "child-emitting Shoot requires positive numShots"
                );
                Some(Emission::Child(ids[0]))
            }
            K::Explode if matches!(fields.get(&F::NumShots), Some(Value::Integer(0))) => {
                ensure!(
                    !fields.contains_key(&F::ProjectileId) && !e.attrs.contains_key("arcDegrees"),
                    "terminal Explode cannot contain projectile configuration"
                );
                Some(Emission::Terminal)
            }
            K::Shoot | K::Explode => {
                let Value::Projectile(id) = fields[&F::ProjectileId] else {
                    unreachable!()
                };
                Some(Emission::Projectile(id))
            }
            _ => None,
        };
        let announcement = if kind == K::OryxTaunt {
            Some(Arc::new(self.announcement()?))
        } else {
            None
        };
        Ok(Behavior {
            local: self.next_local(),
            kind,
            emission,
            announcement,
            bucket: e.optional("bucket", |s| self.link.symbol(s))?,
            fields,
            source: e.source.clone(),
        })
    }
    fn announcement(&self) -> Result<AnnouncementRules> {
        let name = &self.link.content.objects[&self.object].name;
        let list = *self.link.string_lists.get(name).with_context(|| {
            format!("OryxTaunt implicit exact selected-object-name StringList {name:?} missing")
        })?;
        let strings = &self.string_lists[list as usize];
        let admission = strings
            .categories
            .get("new")
            .context("OryxTaunt StringList missing required new category")?
            .clone();
        let terminal = match (
            strings.categories.get("killed"),
            strings.categories.get("death"),
        ) {
            (None, None) => TerminalAnnouncements::BirthOnly,
            (Some(killed), Some(death)) => TerminalAnnouncements::Death {
                killed: killed.clone(),
                death: death.clone(),
            },
            _ => bail!(
                "OryxTaunt terminal categories must supply both killed and death, or neither (BirthOnly)"
            ),
        };
        let channels = std::iter::once(("new", &admission)).chain(match &terminal {
            TerminalAnnouncements::BirthOnly => vec![],
            TerminalAnnouncements::Death { killed, death } => {
                vec![("killed", killed), ("death", death)]
            }
        });
        for (category, texts) in channels {
            for part in texts.iter().flatten() {
                ensure!(
                    matches!(part, TextPart::Literal(_))
                        || (category == "killed" && matches!(part, TextPart::Killer)),
                    "OryxTaunt {category} contains unbound announcement token {part:?}; only killed has explicit KILLER context"
                );
            }
        }
        Ok(AnnouncementRules {
            list,
            source: strings.source.clone(),
            admission,
            terminal,
        })
    }
    fn value(&mut self, kind: BehaviorKind, field: Field, s: &str) -> Result<Value> {
        use Field::*;
        Ok(match field {
            Cooldown | Offset | Duration | Time | InitialDelay | BubbleTime | FlashPeriod
            | RadiusWobblePeriod | ConditionDuration | PoisonDuration => Value::Millis(millis(s)?),
            Angle if kind == BehaviorKind::BackAndForth => Value::Radians(number(s)?),
            Angle | DefaultAngle | ArcDegrees => Value::Radians(number(s)?.to_radians()),
            TurnRate | Rate => Value::Number(number(s)?),
            Predictive if kind == BehaviorKind::Shoot => Value::Number(probability(s)?),
            CooldownJitter if kind == BehaviorKind::TrapTrigger => Value::Millis(millis(s)?),
            MaxPlace => Value::Limit(if s == "-1" { None } else { Some(integer(s)?) }),
            ChildId | ChildIds | ChildCorpseId | ObjectId | TextureSourceId | ProtecteeId
            | TargetId => {
                let ids = self.link.objects(s, false)?;
                self.deps.extend(&ids);
                Value::Objects(ids)
            }
            // Recovered documentation explicitly identifies these fields as group names.
            GroupId | TargetGroups => {
                let ids = self.link.group_names(s)?;
                self.deps.extend(&ids);
                Value::Objects(ids)
            }
            TargetIds | ListenIds | GroupIds => {
                let ids = self.link.objects(s, true)?;
                self.deps.extend(&ids);
                Value::Objects(ids)
            }
            GroundId | GroundTypes | Terrain => Value::Grounds(self.link.grounds(s)?),
            ProjectileId => {
                let id = u8::try_from(integer(s)?)?;
                ensure!(
                    self.link.content.objects[&self.object]
                        .projectiles
                        .contains_key(&id),
                    "unresolved selected projectile {id} on {:#x}",
                    self.object
                );
                Value::Projectile(id)
            }
            Map => Value::Map(
                *self
                    .link
                    .maps
                    .get(s)
                    .with_context(|| format!("unresolved map {s}"))?,
            ),
            Effect
                if matches!(
                    kind,
                    BehaviorKind::ConditionEffectAura | BehaviorKind::ClearConditionEffectAura
                ) =>
            {
                Value::Conditions(
                    names(s)?
                        .into_iter()
                        .map(condition)
                        .collect::<Result<_>>()?,
                )
            }
            Effect | ConditionEffect => {
                ensure!(
                    !s.contains(','),
                    "multiple conditions require explicit separate behaviors"
                );
                Value::Condition(condition(s)?)
            }
            Clockwise | StartWithMinions | DropLoot | GiveXP | TilDeath | ResignPet
            | NoCooldownJitter | CooldownJitter | HideVisual | IgnoreInvisible | Broadcast
            | SendNotification | SetGlobalTag | AvoidWater | CenterObject | Predictive => {
                Value::Bool(match s {
                    "true" => true,
                    "false" => false,
                    _ => bail!("invalid boolean {s}"),
                })
            }
            MaxChildren | NumChildren | NumShots | Min | Max | MaxTargets | FlashRepeats
            | AltTextureId | SoundId => Value::Integer(integer(s)?),
            Prob => Value::Number(probability(s)?),
            Type => Value::Mode(match s {
                "auto" => Mode::Auto,
                "targeted" => Mode::Targeted,
                "Flash" => Mode::Flash,
                _ => bail!("unknown behavior type {s}"),
            }),
            TagName => Value::Symbol(self.link.symbol(s)?),
            Text => Value::TextChoices(
                s.split(';')
                    .map(|s| text_parts(s.trim()))
                    .collect::<Result<_>>()?,
            ),
            ListId => Value::StringList(
                *self
                    .link
                    .string_lists
                    .get(s)
                    .with_context(|| format!("unresolved StringList {s}"))?,
            ),
            Color => Value::Colors(
                names(s)?
                    .iter()
                    .map(|s| {
                        u32::from_str_radix(s.strip_prefix("0x").unwrap_or(s), 16)
                            .context("invalid color")
                    })
                    .collect::<Result<_>>()?,
            ),
            ParticleEffect => {
                ensure!(s == "Nova", "unknown particleEffect {s}");
                Value::Nova
            }
            BlastEffect => Value::Bool(match s {
                "0" => false,
                "1" => true,
                _ => bail!("blastEffect must be 0 or 1"),
            }),
            _ => Value::Number(nonnegative(s)?),
        })
    }
    fn transitions(&mut self) -> Result<()> {
        for (id, e) in self.pending.clone() {
            let result = (|| {
                let state = &self.states[id as usize];
                let parent = state
                    .parent
                    .context("transition on object/order root has no sibling scope")?;
                let targets = names(&e.text)?
                    .into_iter()
                    .map(|name| {
                        self.states[parent as usize]
                            .children
                            .iter()
                            .find(|i| self.states[**i as usize].name == name)
                            .copied()
                            .with_context(|| format!("unresolved sibling state {name:?}"))
                    })
                    .collect::<Result<Vec<_>>>()?;
                let mut predicates = Vec::new();
                for (name, s) in &e.attrs {
                    predicates.push(match name.as_str() {
                        "afterTime" => Predicate::After(millis(s)?),
                        "playerWithin" => {
                            let (radius, min_players) = match s.split_once('|') {
                                Some((radius, count)) => (nonnegative(radius)?, integer(count)?),
                                None => (nonnegative(s)?, 1),
                            };
                            ensure!(min_players > 0, "playerWithin count must be positive");
                            Predicate::PlayerWithin(PlayerProximity {
                                radius,
                                min_players,
                            })
                        }
                        "noPlayerWithin" => Predicate::NoPlayerWithin(nonnegative(s)?),
                        "hitpointsLessThan" => Predicate::HpBelow(probability(s)?),
                        "random" => Predicate::Random(probability(s)?),
                        "exist" | "noneExist" => {
                            let ids = self.link.objects(s, true)?;
                            self.deps.extend(&ids);
                            if name == "exist" {
                                Predicate::Exists(ids)
                            } else {
                                Predicate::NoneExist(ids)
                            }
                        }
                        "onTile" => {
                            let ids = self.link.grounds(s)?;
                            Predicate::OnGround(ids)
                        }
                        "hasTag" | "noTag" | "hasGlobalTag" | "noGlobalTag" => {
                            ensure!(!s.is_empty(), "empty tag predicate");
                            Predicate::Tag {
                                symbol: self.link.symbol(s)?,
                                global: name.contains("Global"),
                                present: name.starts_with("has"),
                            }
                        }
                        "playerSays" => Predicate::PlayerSays(s.into()),
                        _ => bail!("unknown transition attribute {name}"),
                    });
                }
                self.states[id as usize].transitions.push(Transition {
                    targets,
                    predicates,
                    source: e.source.clone(),
                });
                Ok(())
            })();
            result.with_context(|| e.context())?;
        }
        Ok(())
    }
}
fn clear_rule(e: &Element, link: &Link<'_>) -> Result<ClearRule> {
    e.qualified_attributes(&["boss", "route", "portal", "reveal", "retire"])?;
    let boss = link.object(e.required("boss")?)?;
    let portal = e.optional("portal", |s| link.object(s))?;
    let route = match (e.required("route")?, portal) {
        ("scripted", None) => ClearRoute::Scripted,
        ("none", None) => ClearRoute::None,
        ("realm-exit", Some(portal)) => ClearRoute::RealmExit(portal),
        ("victory", Some(portal)) => ClearRoute::Victory(portal),
        (value, _) => bail!("invalid Clear route or portal for {value}"),
    };
    let reveal = e.optional("reveal", |s| link.object(s))?;
    ensure!(
        reveal.is_none() || matches!(route, ClearRoute::Victory(_)),
        "reveal requires a victory route"
    );
    Ok(ClearRule {
        boss,
        route,
        reveal,
        retire: e.optional("retire", |name| link.object(name))?,
        evidence: realm::authored(e)?,
    })
}
fn portal_rule(e: &Element) -> Result<PortalRule> {
    e.qualified_attributes(&["object", "from", "except", "destination", "earnedExit"])?;
    use PortalDestination::*;
    let destination = match e.required("destination")? {
        "nexus" => Nexus,
        "origin-realm" => OriginRealm,
        "random-realm" => RandomRealm,
        "named-realm" => NamedRealm,
        "fresh-kitchen" => FreshKitchen,
        "owned-vault" => OwnedVault,
        "guild-hall" => GuildHall,
        "locked" => Locked,
        value => bail!("unknown portal destination {value}"),
    };
    let earned_exit = e.required("earnedExit")?.parse::<bool>()?;
    ensure!(
        !earned_exit || matches!(destination, OriginRealm | RandomRealm),
        "earned exit requires a dungeon-to-Realm route"
    );
    let from = e.attrs.get("from").cloned();
    let except = match e.attrs.get("except") {
        Some(list) => {
            let scopes: BTreeSet<_> = list.split(',').map(str::to_owned).collect();
            ensure!(
                from.is_none() && !scopes.contains("") && scopes.len() == list.split(',').count(),
                "except requires a unique, nonempty list on an unscoped PortalRoute"
            );
            scopes
        }
        None => BTreeSet::new(),
    };
    Ok(PortalRule {
        from,
        except,
        destination,
        earned_exit,
        evidence: realm::authored(e)?,
    })
}
fn coil_placement(e: &Element) -> Result<CoilPlacement> {
    e.qualified_attributes(&["rooms", "perRoom", "order"])?;
    let rooms = integer(e.required("rooms")?)? as usize;
    let per_room = integer(e.required("perRoom")?)? as usize;
    ensure!(
        rooms > 0 && per_room > 0,
        "CoilPlacement requires positive rooms and perRoom"
    );
    let boss_first = match e.required("order")? {
        "boss-first" => true,
        "entrance-first" => false,
        other => bail!("unknown CoilPlacement order {other}"),
    };
    Ok(CoilPlacement {
        rooms,
        per_room,
        boss_first,
        evidence: realm::authored(e)?,
    })
}
fn validate_portal_contexts(game: &GameData) -> Result<()> {
    let portal = |kind| game.properties[&kind].is_portal();
    let keys: Vec<_> = game
        .definitions
        .values()
        .filter(|d| d.native_service == Some(NativeService::Inventory))
        .filter_map(|d| {
            game.properties[&d.kind]
                .create_object
                .as_ref()
                .map(|name| (d, name))
        })
        .map(|(d, name)| {
            let target = game
                .properties
                .values()
                .find(|p| &p.name == name)
                .with_context(|| {
                    format!("owned key {:#x} references unknown portal {name}", d.kind)
                })?;
            ensure!(
                portal(target.kind),
                "owned key {:#x} target {name} is not a portal",
                d.kind
            );
            Ok((d.kind, target.kind))
        })
        .collect::<Result<_>>()?;
    for world in &game.worlds {
        let map = &game.maps[world.map as usize];
        let from = Some(world.id.as_str());
        let context = format!("World {} map={}", world.id, map.name);
        if world.id == "nexus" {
            ensure!(
                game.portal_route(0x712, from)
                    .with_context(|| format!("{context}: native named Realm entrance"))?
                    .destination
                    == PortalDestination::NamedRealm,
                "{context}: native Realm entrance requires the named-realm route"
            );
        }
        for &(item, target) in &keys {
            game.portal_instance(target, from)
                .with_context(|| format!("{context}: owned key {item:#x}"))?;
        }
        if let Some(unlock) = &world.unlock {
            let context = format!(
                "{context}: owned Incantation Unlock: {}",
                unlock.evidence.origin
            );
            game.admission(
                unlock.item,
                AdmissionRole::Native(NativeService::Inventory),
                &context,
            )?;
            let item = &game.properties[&unlock.item];
            ensure!(
                item.consumable
                    && item.activations == 1
                    && item
                        .unlock_portal
                        .as_ref()
                        .is_some_and(|(_, name)| *name == game.properties[&unlock.locked].name),
                "{context}: expected the selected UnlockPortal activation and locked object binding"
            );
            ensure!(
                world
                    .clear
                    .as_ref()
                    .is_some_and(|clear| clear.route == ClearRoute::Victory(unlock.locked))
                    && game.portal_route(unlock.locked, from)?.destination
                        == PortalDestination::Locked,
                "{context}: unlock source must be the selected locked victory portal"
            );
            game.portal_instance(unlock.opened, from)
                .with_context(|| context.clone())?;
        }
        for &kind in map.objects.iter().filter(|&&kind| portal(kind)) {
            let route = game.portal_route(kind, from).with_context(|| {
                format!("{context}: physical portal {}", game.properties[&kind].name)
            })?;
            ensure!(
                !matches!(
                    route.destination,
                    PortalDestination::Instance(_) | PortalDestination::NamedRealm
                ),
                "{context}: map-placed portal {kind:#x} cannot use a route requiring a live instance or entrance binding"
            );
        }
        let mut pending: Vec<_> = map.objects.iter().copied().collect();
        pending.extend(world.populations.iter().map(|p| p.object));
        pending.extend(world.corner_placement.iter().map(|p| p.kind));
        if let Some(clear) = &world.clear {
            pending.push(clear.boss);
            if let ClearRoute::RealmExit(kind) | ClearRoute::Victory(kind) = clear.route {
                pending.push(kind);
            }
        }
        if world.id == "realm"
            && let Some(realm) = &game.realm
        {
            pending.extend(realm.sites.iter().flat_map(|site| {
                std::iter::once(site.kind)
                    .chain(site.anchor)
                    .chain(site.island.iter().flatten().map(|p| p.kind))
                    .chain(site.interactive_tiles.values().copied())
            }));
            pending.extend(realm.camps.values().flat_map(|camp| {
                std::iter::once(camp.leader).chain(camp.roles.iter().map(|role| role.kind))
            }));
            pending.extend(realm.reproduction.values().flatten().map(|rule| rule.child));
        }
        let mut seen = BTreeSet::new();
        while let Some(kind) = pending.pop() {
            if !seen.insert(kind) {
                continue;
            }
            if portal(kind) {
                game.portal_route(kind, from).with_context(|| {
                    format!("{context}: physical portal {}", game.properties[&kind].name)
                })?;
            }
            let Some(definition) = game.definitions.get(&kind) else {
                continue;
            };
            for b in definition.states.iter().flat_map(|s| &s.behaviors) {
                for (field, value) in &b.fields {
                    let children = |child| -> Result<()> {
                        if portal(child) {
                            let birth = || {
                                format!(
                                    "{context}: portal birth {child:#x} from {}:{} {} {} {:?}",
                                    b.source.file,
                                    b.source.line,
                                    b.source.object,
                                    b.source.path,
                                    b.kind
                                )
                            };
                            let route = game.portal_route(child, from).with_context(birth)?;
                            ensure!(
                                route.destination.can_publish(),
                                "{} requires a publishable route",
                                birth()
                            );
                        }
                        pending.push(child);
                        Ok(())
                    };
                    match value {
                        Value::Map(i) => game.maps[*i as usize]
                            .objects
                            .iter()
                            .copied()
                            .try_for_each(children)?,
                        Value::Objects(kinds) if is_entity_birth_field(*field, b.kind) => {
                            kinds.iter().copied().try_for_each(children)?
                        }
                        _ => (),
                    }
                }
            }
        }
    }
    Ok(())
}

fn is_entity_birth_field(field: Field, kind: BehaviorKind) -> bool {
    matches!(
        field,
        Field::ChildId | Field::ChildIds | Field::ChildCorpseId
    ) || (field == Field::GroupId && kind == BehaviorKind::MakeMinions)
}

fn validate_world_routes(game: &GameData) -> Result<()> {
    for (&kind, rules) in &game.portal_routes {
        for rule in rules {
            let context = format!(
                "PortalRoute kind={kind:#x} from={:?}: {}",
                rule.from, rule.evidence.origin
            );
            let definition = game
                .definitions
                .get(&kind)
                .with_context(|| format!("{context}: missing selected portal definition"))?;
            ensure!(
                game.properties[&kind].is_portal()
                    && matches!(
                        definition.admission_role()?,
                        AdmissionRole::Scripted | AdmissionRole::Native(NativeService::Portal)
                    ),
                "{context}: not a selected physical portal"
            );
            use PortalDestination::*;
            let required_worlds: &[&str] = match &rule.destination {
                Nexus => &["nexus"],
                RandomRealm | OriginRealm | NamedRealm => &["realm"],
                FreshKitchen => &["kitchen"],
                OwnedVault => &["vault"],
                GuildHall => &[
                    "guild-hall-1",
                    "guild-hall-2",
                    "guild-hall-3",
                    "guild-hall-4",
                ],
                Instance(template) => &[template],
                Locked => &[],
            };
            for target in required_worlds {
                ensure!(
                    game.world_template(target).is_some(),
                    "{context}: missing destination World {target}"
                );
            }
            ensure!(
                (rule.destination == Locked) == (kind == crate::endgame::LOCKED_CELLAR),
                "{context}: only the selected locked Cellar portal may use the explicit locked route"
            );
            for from in rule.from.iter().chain(&rule.except) {
                ensure!(
                    game.world_template(from).is_some(),
                    "{context}: unknown origin world {from}"
                );
            }
            if let Some(from) = &rule.from {
                ensure!(
                    rules
                        .iter()
                        .all(|r| r.from.is_some() || r.except.contains(from)),
                    "{context}: overlapping PortalRoute scopes"
                );
            }
        }
    }
    for world in &game.worlds {
        if let Some(placement) = &world.corner_placement {
            let context = format!(
                "World {} CornerPlacement: {}",
                world.id, placement.evidence.origin
            );
            let definition = game.admission(placement.kind, AdmissionRole::Scripted, &context)?;
            ensure!(
                definition.disposition == Disposition::Passive
                    && game.properties[&placement.kind].destructible,
                "{context}: corner placement requires a selected passive destructible"
            );
        }
        if let Some(instance) = &world.instance {
            let context = format!("World {} Instance: {}", world.id, instance.evidence.origin);
            ensure!(
                world.clear.is_some(),
                "{context}: instance requires explicit Clear intent"
            );
        }
        let lab = matches!(&game.maps[world.map as usize].geometry,
            geometry::Geometry::Generator(generator) if generator.kind == geometry::GeneratorKind::Lab);
        ensure!(
            lab == world.coils.is_some(),
            "World {}: Lab generator requires exactly one CoilPlacement; other worlds cannot declare it",
            world.id
        );
        if let Some(clear) = &world.clear {
            let context = format!("World {} Clear: {}", world.id, clear.evidence.origin);
            let boss = game.admission(clear.boss, AdmissionRole::Scripted, &context)?;
            let victory = boss
                .states
                .iter()
                .flat_map(|s| &s.behaviors)
                .any(|b| b.kind == BehaviorKind::VictoryAtDeath);
            let scripted_portal = boss.states.iter().flat_map(|s| &s.behaviors).any(|b| {
                matches!(b.kind, BehaviorKind::Metamorphose | BehaviorKind::MetamorphoseAtDeath | BehaviorKind::ReleaseSpawnAtDeath)
                    && [Field::ChildId, Field::ChildIds].iter().any(|field| matches!(b.fields.get(field), Some(Value::Objects(ids)) if ids.iter().any(|kind| game.properties[kind].class == "Portal")))
            });
            ensure!(
                match clear.route {
                    ClearRoute::Scripted => !victory,
                    ClearRoute::Victory(_) => victory && !scripted_portal,
                    ClearRoute::None => !scripted_portal,
                    ClearRoute::RealmExit(_) => !scripted_portal && !victory,
                },
                "{context}: clear route contradicts or duplicates the linked actor portal declaration"
            );
            let portal = |kind| -> Result<&PortalRule> {
                let route = game
                    .portal_route(kind, Some(&world.id))
                    .with_context(|| context.clone())?;
                ensure!(
                    route.destination.can_publish(),
                    "{context}: portal {kind:#x} requires a publishable route"
                );
                Ok(route)
            };
            match clear.route {
                ClearRoute::RealmExit(kind) => ensure!(
                    portal(kind)?.destination == PortalDestination::OriginRealm,
                    "{context}: portal {kind:#x} does not return to the originating Realm"
                ),
                ClearRoute::Victory(kind) => {
                    portal(kind)?;
                }
                _ => (),
            }
            if let Some(retire) = clear.retire {
                portal(retire)?;
                ensure!(
                    Some(retire) != clear.reveal
                        && game.maps[world.map as usize].objects.contains(&retire),
                    "{context}: retire portal {retire:#x} must be a distinct selected map placement"
                );
            }
            if let Some(kind) = clear.reveal {
                portal(kind)?;
                ensure!(
                    game.maps[world.map as usize].objects.contains(&kind),
                    "{context}: reveal portal {kind:#x} is not placed by the selected map"
                );
            }
        }
    }
    Ok(())
}
fn world_service<T>(e: &Element, world: &str, value: T) -> Result<WorldService<T>> {
    e.qualified_attributes(&[])?;
    e.leaf()?;
    let RowOrigin {
        provenance,
        origin,
        reason,
    } = e.qualification("world service ")?;
    let mut source = e.source.clone();
    source.path = format!("World/{world}/{}", e.tag);
    Ok(WorldService {
        value,
        provenance,
        origin,
        reason,
        source,
    })
}
impl WorldServices {
    fn insert(&mut self, e: &Element, world: &str) -> Result<()> {
        fn set<T>(slot: &mut Option<WorldService<T>>, value: WorldService<T>) -> Result<()> {
            ensure!(slot.is_none(), "duplicate world service");
            *slot = Some(value);
            Ok(())
        }
        let result = (|| match e.tag.as_str() {
            "Visibility" => set(
                &mut self.visibility,
                world_service(
                    e,
                    world,
                    match e.text.as_str() {
                        "Full" => Visibility::Full,
                        "Path" => Visibility::Path,
                        "LineOfSight" => Visibility::LineOfSight,
                        s => bail!("unknown Visibility {s:?}"),
                    },
                )?,
            ),
            "Difficulty" => {
                let n = integer(&e.text)?;
                ensure!(n <= 5, "Difficulty outside authored supported range 0..5");
                set(&mut self.difficulty, world_service(e, world, n as u8)?)
            }
            "Background" => set(
                &mut self.background,
                world_service(
                    e,
                    world,
                    match integer(&e.text)? {
                        0 => Background::None,
                        1 => Background::Stars,
                        2 => Background::Nexus,
                        n => bail!("Background {n} has no supported selected-client factory"),
                    },
                )?,
            ),
            "UsesBreath" => {
                let n = number(&e.text)?;
                ensure!(n > 0., "UsesBreath must be positive");
                set(&mut self.uses_breath, world_service(e, world, n)?)
            }
            "AllowTeleport" => set(
                &mut self.allow_teleport,
                world_service(e, world, e.text.parse()?)?,
            ),
            "SummonLimit" => {
                let cap = integer(&e.text)? as usize;
                ensure!(
                    (1..=4096).contains(&cap),
                    "SummonLimit outside supported range 1..=4096"
                );
                set(&mut self.summon_limit, world_service(e, world, cap)?)
            }
            "HelloNotification" => {
                ensure!(
                    e.text == "showKeyUI",
                    "unknown HelloNotification {:?}",
                    e.text
                );
                set(
                    &mut self.hello_notification,
                    world_service(e, world, HelloNotification::ShowKeyUi)?,
                )
            }
            _ => bail!("unknown World child {}", e.tag),
        })();
        result.with_context(|| format!("{} world={world} service={}", e.context(), e.tag))
    }
}
fn server_property(e: &Element, rule: &mut PopulationRule, xp: &mut f64) -> Result<()> {
    match e.tag.as_str() {
        "SpawnProb" => rule.weight = Some(probability(&e.text)?),
        "XpMult" => *xp = nonnegative(&e.text)?,
        "PerRealmMax" => {
            let max = integer(&e.text)?;
            ensure!(
                max > 0 && max <= 4096,
                "invalid bounded per-world population"
            );
            rule.per_world_max = Some(max);
        }
        "Terrain" => {
            rule.terrain = Some(terrain(&e.text)?);
        }
        "Spawn" => {
            ensure!(e.text.is_empty(), "unexpected Spawn text");
            let mut fields = BTreeMap::new();
            for c in &e.children {
                ensure!(
                    ["Mean", "StdDev", "Min", "Max"].contains(&c.tag.as_str()),
                    "unknown Spawn field {}",
                    c.tag
                );
                ensure!(
                    fields
                        .insert(c.tag.as_str(), integer(&c.text)? as usize)
                        .is_none(),
                    "duplicate Spawn field {}",
                    c.tag
                );
            }
            let field = |name: &str| {
                fields
                    .get(name)
                    .copied()
                    .with_context(|| format!("incomplete Spawn: missing {name}"))
            };
            let group = super::super::SpawnGroup {
                mean: field("Mean")?,
                deviation: field("StdDev")?,
                min: field("Min")?,
                max: field("Max")?,
            };
            ensure!(
                group.min > 0
                    && group.min <= group.mean
                    && group.mean <= group.max
                    && group.max <= 32,
                "invalid bounded Spawn group"
            );
            rule.group = Some(group);
        }
        _ => bail!("unknown server property {}", e.tag),
    }
    Ok(())
}
fn loot_syntax(e: &Element) -> Result<()> {
    e.qualified_attributes(&["prob", "threshold", "min", "max", "tier"])?;
    e.leaf()
}
fn loot(e: &Element, link: &Link<'_>) -> Result<LootRow> {
    loot_syntax(e)?;
    ensure!(
        !e.text.is_empty(),
        "empty {} row requires item/category text; selected-client container markers are not server loot rows",
        e.tag
    );
    let (category, tier, pool) = if let Some(tier) = e.attrs.get("tier") {
        let tier = u8::try_from(integer(tier)?)?;
        let category = match e.text.as_str() {
            "Weapon" => LootCategory::Weapon,
            "Ability" => LootCategory::Ability,
            "ClassSpecial" => LootCategory::ClassSpecial,
            "Armor" => LootCategory::Armor,
            "Ring" => LootCategory::Ring,
            "Potion" => LootCategory::Potion,
            _ => bail!("unknown tier pool {}", e.text),
        };
        let slots: &[i32] = match category {
            LootCategory::Weapon => &[1, 2, 3, 8, 17],
            LootCategory::Ability | LootCategory::ClassSpecial => {
                &[4, 5, 11, 12, 13, 15, 16, 18, 19, 20, 21, 22, 23]
            }
            LootCategory::Armor => &[6, 7, 14],
            LootCategory::Ring => &[9],
            LootCategory::Potion => &[10],
        };
        let pool = link
            .content
            .objects
            .values()
            .filter(|o| o.tier == Some(tier) && slots.contains(&o.slot_type))
            .map(|o| o.kind)
            .collect::<Vec<_>>();
        (Some(category), Some(tier), pool)
    } else {
        (None, None, vec![link.object(&e.text)?])
    };
    ensure!(!pool.is_empty(), "empty selected tier pool {}", e.text);
    let min = e.optional("min", integer)?.unwrap_or(0);
    let max = e.optional("max", integer)?;
    ensure!(max.is_none_or(|max| min <= max), "loot min exceeds max");
    let row_origin = match (
        e.attrs.get("provenance"),
        e.attrs.get("origin"),
        e.attrs.get("reason"),
    ) {
        (None, None, None) => None,
        (Some(p), Some(origin), Some(reason))
            if p == "authored" && !origin.trim().is_empty() && !reason.trim().is_empty() =>
        {
            Some(RowOrigin {
                provenance: Provenance::Authored,
                origin: origin.clone(),
                reason: reason.clone(),
            })
        }
        _ => bail!("authored loot row needs provenance, origin and reason"),
    };
    Ok(LootRow {
        row_origin,
        category,
        tier,
        ownership: if e.tag == "Loot" {
            LootOwnership::Public
        } else {
            LootOwnership::Private
        },
        pool,
        probability: e.optional("prob", probability)?.unwrap_or(1.),
        threshold: e.optional("threshold", probability)?.unwrap_or(0.),
        min,
        max,
        source: e.source.clone(),
    })
}

fn landmark_chest(e: &Element, link: &Link<'_>) -> Result<LandmarkChest> {
    e.qualified_attributes(&[
        "landmark",
        "container",
        "platform",
        "ceilingMin",
        "ceilingMax",
    ])?;
    ensure!(
        e.required("provenance")? == "authored",
        "landmark chest rows are authored reconstructions"
    );
    let landmark = link.object(e.required("landmark")?)?;
    let ceiling = match (e.attrs.get("ceilingMin"), e.attrs.get("ceilingMax")) {
        (None, None) => None,
        (Some(low), Some(high)) => {
            let (low, high) = (integer(low)?, integer(high)?);
            ensure!(0 < low && low <= high, "invalid chest success ceiling");
            Some((low, high))
        }
        _ => bail!("chest success ceiling needs ceilingMin and ceilingMax"),
    };
    let rows = e
        .children
        .iter()
        .map(|c| {
            ensure!(c.tag == "Loot", "landmark chest rows must be public Loot");
            let mut row = loot(c, link).with_context(|| c.context())?;
            ensure!(
                row.threshold == 0. && row.min == 0 && row.max.is_none(),
                "{}: chest rows have no contributor threshold/min/max",
                c.context()
            );
            row.source.object = link.content.get(landmark)?.name.clone();
            row.source.path = "LandmarkChest".into();
            Ok(row)
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(!rows.is_empty(), "empty landmark chest");
    Ok(LandmarkChest {
        landmark,
        container: link.object(e.required("container")?)?,
        platform: e.optional("platform", |name| link.object(name))?,
        ceiling,
        rows,
        provenance: Provenance::Authored,
        origin: e.required("origin")?.to_owned(),
        reason: e.required("reason")?.to_owned(),
        source: e.source.clone(),
    })
}

#[cfg(test)]
pub(super) fn compile(content: &Content, bytes: BTreeMap<String, Vec<u8>>) -> Result<GameData> {
    compile_at(content, bytes, None)
}
pub(super) fn compile_at(
    content: &Content,
    bytes: BTreeMap<String, Vec<u8>>,
    data_root: Option<&Path>,
) -> Result<GameData> {
    let input = |name: &str| {
        bytes
            .get(name)
            .with_context(|| format!("missing required input {name}"))
    };
    let manifest = parse(MANIFEST, input(MANIFEST)?)?;
    ensure!(manifest.tag == "Game", "expected Game manifest");
    manifest.attributes(&["schema"])?;
    ensure!(
        manifest.required("schema")? == "1",
        "unsupported Game schema"
    );
    let mut link = Link::new(content)?;
    let mut used = BTreeSet::from([MANIFEST.to_owned()]);
    let mut raw = BTreeMap::new();
    let mut geometry_raw = Vec::new();
    let mut string_lists = Vec::new();
    for e in &manifest.children {
        let result = (|| {
            match e.tag.as_str() {
                "Include" => {
                    e.attributes(&["file"])?;
                    e.leaf()?;
                    let file = include_path(e.required("file")?)?;
                    ensure!(used.insert(file.clone()), "duplicate Include {file}");
                    let doc = parse(&file, input(&file)?)?;
                    if doc.tag == "StringLists" {
                        doc.attributes(&[])?;
                        for e in &doc.children {
                            let list = string_list(e).with_context(|| e.context())?;
                            ensure!(
                                link.string_lists
                                    .insert(list.name.clone(), string_lists.len() as u32)
                                    .is_none(),
                                "{}: duplicate StringList {}",
                                e.context(),
                                list.name
                            );
                            string_lists.push(list);
                        }
                        return Ok(());
                    }
                    ensure!(
                        doc.tag == "Objects",
                        "expected Objects or StringLists in {file}"
                    );
                    doc.attributes(&[])?;
                    for mut o in doc.children {
                        let result = (|| {
                            ensure!(o.tag == "Object", "unknown {} in Objects", o.tag);
                            let id = super::super::kind(o.required("type")?)?;
                            let prop = content.get(id)?;
                            ensure!(
                                o.required("id")? == prop.name,
                                "selected identity mismatch {id:#x}: expected {}",
                                prop.name
                            );
                            fn stamp(e: &mut Element, object: &str, path: &str) {
                                e.source.object = object.into();
                                e.source.path = path.into();
                                for c in &mut e.children {
                                    let path = if c.tag == "State" {
                                        format!("{path}/{}", c.attr("id").unwrap_or("<missing>"))
                                    } else if c.tag == "DefineEmbed" {
                                        format!(
                                            "{path}/embed:{}",
                                            c.attr("name").unwrap_or("<missing>")
                                        )
                                    } else {
                                        path.into()
                                    };
                                    stamp(c, object, &path);
                                }
                            }
                            stamp(&mut o, &prop.name, "@root");
                            syntax(&o)?;
                            for g in o.children.iter().filter(|c| c.tag == "Group") {
                                ensure!(!g.text.is_empty(), "empty Group");
                                link.groups.entry(g.text.clone()).or_default().insert(id);
                            }
                            ensure!(
                                raw.insert(id, o.clone()).is_none(),
                                "duplicate selected definition {id:#x}"
                            );
                            Ok(())
                        })();
                        result.with_context(|| o.context())?;
                    }
                }
                "Map" => geometry_raw.push(e.clone()),
                "Geometry" => {
                    e.attributes(&["file"])?;
                    e.leaf()?;
                    let file = include_path(e.required("file")?)?;
                    used.insert(file.clone());
                    let doc = parse(&file, input(&file)?)?;
                    ensure!(doc.tag == "Geometry", "expected Geometry recipe library");
                    doc.attributes(&["schema"])?;
                    ensure!(doc.required("schema")? == "1", "unknown Geometry schema");
                    geometry_raw.extend(doc.children);
                }
                "Root" | "World" | "LandmarkChest" | "RealmRules" | "PortalRoute" => (),
                "Evidence" => {
                    e.attributes(&["file"])?;
                    e.leaf()?;
                    let file = e.required("file")?;
                    safe_path(file)?;
                    input(file)?;
                    used.insert(file.into());
                }
                _ => bail!("unknown Game element {}", e.tag),
            }
            Ok(())
        })();
        result.with_context(|| e.context())?;
    }
    for (i, e) in geometry_raw.iter().enumerate() {
        let name = super::geometry::name(e)?;
        ensure!(
            link.maps.insert(name.to_owned(), i as u32).is_none(),
            "duplicate geometry resource {name}"
        );
    }
    let mut geometry_inputs = BTreeMap::new();
    let maps = super::geometry::compile(
        &geometry_raw,
        &link,
        &bytes,
        &mut used,
        data_root,
        &mut geometry_inputs,
    )?;
    let mut roots = BTreeSet::new();
    let mut root_contexts = BTreeMap::new();
    let mut root_declarations = BTreeSet::new();
    let mut worlds = Vec::new();
    let mut portal_routes: BTreeMap<u16, Vec<PortalRule>> = BTreeMap::new();
    let insert_portal =
        |routes: &mut BTreeMap<u16, Vec<PortalRule>>, kind, rule: PortalRule| -> Result<()> {
            let rules = routes.entry(kind).or_default();
            ensure!(
                !rules.iter().any(|r| r.from == rule.from),
                "duplicate PortalRoute ownership kind={kind:#x} from={:?}",
                rule.from
            );
            rules.push(rule);
            Ok(())
        };
    let mut world_ids = BTreeSet::new();
    let mut landmark_chests = BTreeMap::new();
    let mut realm_rules = None;
    for e in &manifest.children {
        let result = (|| {
            match e.tag.as_str() {
                "Root" => {
                    e.attributes(&["object"])?;
                    e.leaf()?;
                    let id = link.object(e.required("object")?)?;
                    ensure!(root_declarations.insert(id), "duplicate Root {id:#x}");
                    roots.insert(id);
                    root_contexts.insert(id, format!("{} Root object={id:#x}", e.context()));
                }
                "World" => {
                    e.attributes(&["id", "map", "clearKey", "exitKey"])?;
                    ensure!(e.text.is_empty(), "unexpected World text");
                    let id = e.required("id")?.to_owned();
                    ensure!(world_ids.insert(id.clone()), "duplicate world {id}");
                    let map = *link
                        .maps
                        .get(e.required("map")?)
                        .context("unresolved world map")?;
                    roots.extend(&maps[map as usize].objects);
                    for kind in &maps[map as usize].objects {
                        root_contexts.insert(
                            *kind,
                            format!(
                                "{} world={id} map={} entity placement",
                                e.context(),
                                maps[map as usize].name
                            ),
                        );
                    }
                    let mut populations = Vec::new();
                    let mut services = WorldServices::default();
                    let mut instance = None;
                    let mut clear = None;
                    let mut coils = None;
                    let mut corner_placement = None;
                    let mut required_actors = Vec::new();
                    let mut unlock = None;
                    for c in &e.children {
                        c.leaf()?;
                        match c.tag.as_str() {
                            "Population" => {
                                c.attributes(&["object", "min", "max"])?;
                                let object = link.object(c.required("object")?)?;
                                let min = integer(c.required("min")?)?;
                                let max = integer(c.required("max")?)?;
                                ensure!(
                                    min <= max && max > 0 && max <= 4096,
                                    "invalid bounded world population"
                                );
                                ensure!(
                                    !populations.iter().any(|p: &Population| p.object == object),
                                    "duplicate world population"
                                );
                                roots.insert(object);
                                root_contexts.insert(
                                    object,
                                    format!("{} world={id} Population birth", c.context()),
                                );
                                populations.push(Population { object, min, max });
                            }
                            "Instance" => (|| -> Result<()> {
                                c.qualified_attributes(&["entrance", "name"])?;
                                ensure!(instance.is_none(), "duplicate Instance");
                                instance = Some(InstanceRule {
                                    entrance: link.object(c.required("entrance")?)?,
                                    name: c.required("name")?.into(),
                                    evidence: realm::authored(c)?,
                                });
                                Ok(())
                            })()
                            .with_context(|| c.context())?,
                            "CoilPlacement" => {
                                ensure!(coils.is_none(), "{} duplicate CoilPlacement", c.context());
                                coils = Some(coil_placement(c).with_context(|| c.context())?);
                            }
                            "Clear" => {
                                ensure!(clear.is_none(), "{} duplicate Clear", c.context());
                                clear = Some(clear_rule(c, &link).with_context(|| c.context())?);
                            }
                            "CornerPlacement" => {
                                c.qualified_attributes(&["object"])?;
                                let kind = link.object(c.required("object")?)?;
                                ensure!(
                                    corner_placement.is_none(),
                                    "{} duplicate CornerPlacement",
                                    c.context()
                                );
                                corner_placement = Some(CornerPlacement {
                                    kind,
                                    evidence: realm::authored(c)?,
                                });
                                roots.insert(kind);
                                root_contexts.insert(
                                    kind,
                                    format!("{} World {id} CornerPlacement", c.context()),
                                );
                            }
                            "Unlock" => {
                                c.qualified_attributes(&["item", "locked", "opened"])?;
                                ensure!(unlock.is_none(), "{} duplicate Unlock", c.context());
                                let rule = UnlockRule {
                                    item: link.object(c.required("item")?)?,
                                    locked: link.object(c.required("locked")?)?,
                                    opened: link.object(c.required("opened")?)?,
                                    evidence: realm::authored(c)?,
                                };
                                for kind in [rule.item, rule.locked, rule.opened] {
                                    roots.insert(kind);
                                    root_contexts
                                        .insert(kind, format!("{} World {id} Unlock", c.context()));
                                }
                                unlock = Some(rule);
                            }
                            "RequiredActor" => {
                                c.attributes(&["object"])?;
                                let kind = link.object(c.required("object")?)?;
                                ensure!(
                                    maps[map as usize].objects.contains(&kind)
                                        && !required_actors.contains(&kind),
                                    "{} World {id} RequiredActor must reference a unique selected map placement",
                                    c.context()
                                );
                                required_actors.push(kind);
                            }
                            _ => services.insert(c, &id)?,
                        }
                    }
                    let clear_key: Option<ProgressKey> = e
                        .optional("clearKey", |s| {
                            serde_json::from_value(serde_json::Value::String(s.into()))
                        })
                        .context("unknown stable clear key")?;
                    if let Some(key) = clear_key {
                        ensure!(
                            key.world() == id,
                            "stable clear key belongs to {}, not {id}",
                            key.world()
                        );
                    }
                    let exit_key: Option<ExitKey> = e
                        .optional("exitKey", |s| {
                            serde_json::from_value(serde_json::Value::String(s.into()))
                        })
                        .context("unknown stable exit key")?;
                    if let Some(key) = exit_key {
                        ensure!(
                            serde_json::to_value(key)?
                                .as_str()
                                .context("exit key must serialize as string")?
                                .replace('_', "-")
                                == id,
                            "stable exit key/world mismatch"
                        );
                    }
                    ensure!(
                        clear_key.is_none() || clear.is_some(),
                        "{} clearKey requires an explicit Clear intent",
                        e.context()
                    );
                    let route_kinds =
                        instance
                            .iter()
                            .map(|r| r.entrance)
                            .chain(clear.iter().flat_map(|r| {
                                let portal = match r.route {
                                    ClearRoute::RealmExit(kind) | ClearRoute::Victory(kind) => {
                                        Some(kind)
                                    }
                                    _ => None,
                                };
                                std::iter::once(r.boss).chain(portal).chain(r.reveal)
                            }));
                    for kind in route_kinds {
                        roots.insert(kind);
                        root_contexts
                            .insert(kind, format!("{} world={id} route reference", e.context()));
                    }
                    if let Some(rule) = &instance {
                        insert_portal(
                            &mut portal_routes,
                            rule.entrance,
                            PortalRule {
                                from: None,
                                except: BTreeSet::new(),
                                destination: PortalDestination::Instance(id.clone()),
                                earned_exit: false,
                                evidence: rule.evidence.clone(),
                            },
                        )?;
                    }
                    worlds.push(World {
                        id,
                        map,
                        populations,
                        services,
                        clear_key,
                        exit_key,
                        instance,
                        clear,
                        coils,
                        corner_placement,
                        required_actors,
                        unlock,
                    });
                }
                "PortalRoute" => {
                    e.leaf()?;
                    let kind = link.object(e.required("object")?)?;
                    insert_portal(&mut portal_routes, kind, portal_rule(e)?)?;
                }
                "RealmRules" => {
                    ensure!(realm_rules.is_none(), "duplicate RealmRules");
                    let rules = realm::compile(e, &link)?;
                    for site in &rules.sites {
                        for kind in site
                            .island
                            .iter()
                            .flatten()
                            .map(|p| p.kind)
                            .chain(site.interactive_tiles.values().copied())
                        {
                            roots.insert(kind);
                            root_contexts.insert(
                                kind,
                                format!("{} NamedSite {:#x} placement", e.context(), site.kind),
                            );
                        }
                    }
                    realm_rules = Some(rules);
                }
                "LandmarkChest" => {
                    let chest = landmark_chest(e, &link)?;
                    ensure!(
                        landmark_chests.insert(chest.landmark, chest).is_none(),
                        "duplicate landmark chest"
                    );
                }
                _ => (),
            }
            Ok(())
        })();
        result.with_context(|| e.context())?;
    }
    ensure!(!roots.is_empty(), "no production roots");
    let mut definitions = BTreeMap::new();
    for (id, e) in &raw {
        let result = (|| {
            let disposition = match e.required("disposition")? {
                "active" => Disposition::Active,
                "passive" => Disposition::Passive,
                "native" => Disposition::Native,
                v => bail!("unknown disposition {v}"),
            };
            let RowOrigin {
                provenance,
                origin,
                reason,
            } = e.qualification("")?;
            let prop = content.get(*id)?;
            ensure!(
                disposition == Disposition::Native
                    || !matches!(
                        prop.class.as_str(),
                        "Equipment" | "Dye" | "Player" | "Projectile"
                    ),
                "selected property-only class {} requires its explicit native service",
                prop.class
            );
            let native_service = if disposition == Disposition::Native {
                let service = native_service(e.required("nativeService")?, prop, content)?;
                ensure!(
                    e.children.is_empty(),
                    "native definition cannot contain server payload"
                );
                Some(service)
            } else {
                ensure!(
                    !e.attrs.contains_key("nativeService"),
                    "nativeService requires native disposition"
                );
                None
            };
            ensure!(
                disposition != Disposition::Passive
                    || (prop.projectiles.is_empty()
                        && (matches!(prop.class.as_str(), "Wall" | "GameObject") || *id == 0xd39)),
                "passive disposition requires selected scenery or explicitly disabled trap"
            );
            ensure!(
                !matches!(native_service, Some(NativeService::Deployment(_)))
                    || provenance != Provenance::Recovered,
                "native deployment binding must be qualified authored/corrected, not a recovered activation target"
            );
            check_templates(e, &[], &mut vec![], &mut BTreeSet::new())?;
            let mut expanded = e.clone();
            expanded.children = expand(e, &[], &BTreeMap::new(), &mut vec![], &mut 0)?;
            let mut loot_rows = Vec::new();
            let mut population = PopulationRule::default();
            // This default is selected XML, never the former Rust balance tables.
            let mut xp_multiplier = f64::from(prop.xp_multiplier);
            let mut property_fields = BTreeSet::new();
            let mut runtime = Vec::new();
            for mut child in expanded.children {
                child.source.object = e.source.object.clone();
                match child.tag.as_str() {
                    "Group" => (),
                    "SpawnProb" | "PerRealmMax" | "Terrain" | "XpMult" | "Spawn" => {
                        ensure!(
                            property_fields.insert(child.tag.clone()),
                            "duplicate server property {}",
                            child.tag
                        );
                        server_property(&child, &mut population, &mut xp_multiplier)
                            .with_context(|| child.context())?;
                    }
                    "Loot" | "SoulboundLoot" => {
                        loot_rows.push(loot(&child, &link).with_context(|| child.context())?)
                    }
                    _ => runtime.push(child),
                }
            }
            expanded.children = runtime;
            ensure!(
                disposition == Disposition::Active || expanded.children.is_empty(),
                "passive/native definition contains executable rules"
            );
            ensure!(
                disposition != Disposition::Active || !expanded.children.is_empty(),
                "active definition has no executable rules"
            );
            let mut b = Builder {
                link: &link,
                string_lists: &string_lists,
                object: *id,
                states: vec![],
                pending: vec![],
                deps: BTreeSet::new(),
                local: 0,
            };
            let root = b.state(&expanded, None, "@root", "@root")?;
            b.transitions()?;
            ensure!(
                disposition != Disposition::Active
                    || b.states.iter().any(|s| !s.behaviors.is_empty()),
                "active definition has no effects"
            );
            b.deps.extend(loot_rows.iter().flat_map(|r| &r.pool));
            definitions.insert(
                *id,
                Definition {
                    kind: *id,
                    disposition,
                    native_service,
                    provenance,
                    reason,
                    origin,
                    root,
                    states: b.states,
                    loot: loot_rows,
                    population,
                    xp_multiplier,
                    dependencies: b.deps,
                    source: e.source.clone(),
                },
            );
            Ok(())
        })();
        result.with_context(|| e.context())?;
    }
    // Wire-visible projectile/create/successor identities come only from the selected client.
    let mut bindings = BTreeSet::new();
    for definition in definitions.values() {
        if let Some(NativeService::Deployment(kind)) = definition.native_service {
            ensure!(
                bindings.insert(kind),
                "{}:{} object={}: ambiguous {kind:?} deployment bindings",
                definition.source.file,
                definition.source.line,
                definition.source.object
            );
        }
    }
    let mut deployment_activations = BTreeMap::new();
    let mut property_deps: BTreeMap<u16, BTreeSet<u16>> = BTreeMap::new();
    let mut property_births = Vec::new();
    for (i, file) in ["objects.xml", "grounds.xml"].iter().enumerate() {
        let data = input(file)?;
        ensure!(
            hash(data) == content.xml_sha256[i],
            "selected-client fingerprint mismatch for {file}"
        );
        used.insert((*file).into());
        if i == 0 {
            let selected = parse(file, data)?;
            for o in selected.children {
                let id = super::super::kind(o.required("type")?)?;
                let mut deps = BTreeSet::new();
                let property = content.get(id)?;
                let mut scoped_targets = BTreeSet::new();
                for (activation, c) in o
                    .children
                    .iter()
                    .filter(|c| c.tag == "Activate")
                    .enumerate()
                {
                    let Some(kind) = property
                        .abilities
                        .get(activation)
                        .and_then(DeploymentKind::of_ability)
                    else {
                        continue;
                    };
                    let declared_item = definitions
                        .get(&id)
                        .is_some_and(|d| d.native_service == Some(NativeService::Inventory));
                    if !(declared_item
                        || (kind == DeploymentKind::SpiritBomb && bindings.contains(&kind)))
                    {
                        continue;
                    }
                    let binding = deployment_definition(&definitions, kind).with_context(|| {
                        format!("{} item={id:#x} {}", c.context(), property.name)
                    })?;
                    ensure!(
                        matches!(
                            (kind, c.text.as_str()),
                            (DeploymentKind::Trap, "Trap")
                                | (DeploymentKind::Decoy, "Decoy")
                                | (DeploymentKind::SpiritBomb, "Pet")
                        ),
                        "{} item={id:#x} {}: selected deployment activation disagrees with parsed ability",
                        c.context(),
                        property.name
                    );
                    if kind == DeploymentKind::SpiritBomb {
                        ensure!(
                            c.text == "Pet"
                                && link.object(c.required("objectId")?)? == binding.kind,
                            "{} item={id:#x} {}: selected SpiritBomb Pet target disagrees with XML deployment binding {:#x}",
                            c.context(),
                            property.name,
                            binding.kind
                        );
                    }
                    if declared_item {
                        let mut source = c.source.clone();
                        source.object = property.name.clone();
                        source.path = format!("@Activate[{activation}]");
                        deployment_activations
                            .entry(id)
                            .or_insert_with(Vec::new)
                            .push(DeploymentActivation {
                                activation,
                                kind,
                                target: binding.kind,
                                source,
                            });
                        deps.insert(binding.kind);
                        scoped_targets.insert(c.source.offset);
                    }
                }
                for c in &o.children {
                    let name = if c.tag == "Projectile" {
                        c.children
                            .iter()
                            .find(|c| c.tag == "ObjectId")
                            .map(|c| c.text.as_str())
                    } else if c.tag == "SuccessorId" {
                        Some(c.text.as_str())
                    } else if c.tag == "Activate" && c.text == "Create" {
                        Some(c.required("id")?)
                    } else if c.tag == "Activate" && matches!(c.text.as_str(), "Pet" | "PermaPet") {
                        Some(c.required("objectId")?)
                    } else {
                        None
                    };
                    if let Some(name) = name {
                        let target = link.object(name).with_context(|| c.context())?;
                        deps.insert(target);
                        if c.tag == "Activate" && !scoped_targets.contains(&c.source.offset) {
                            property_births.push((id, target, c.source.clone()));
                        }
                    }
                }
                for kind in content.get(id)?.equipment.iter().filter(|kind| **kind >= 0) {
                    let kind = u16::try_from(*kind).context("selected equipment kind overflow")?;
                    content.get(kind)?;
                    deps.insert(kind);
                }
                property_deps.insert(id, deps);
            }
        }
    }
    let mut reachable = BTreeSet::new();
    let mut queue: Vec<_> = root_contexts.into_iter().collect();
    for chest in landmark_chests.values() {
        let context = format!(
            "{}:{} landmark chest {:#x}",
            chest.source.file, chest.source.line, chest.landmark
        );
        ensure!(
            definitions
                .get(&chest.container)
                .map(|d: &Definition| d.native_service)
                == Some(Some(NativeService::Container))
                && definitions.contains_key(&chest.landmark),
            "{context}: landmark/container lacks its declared admission"
        );
        if let Some(kind) = chest.platform {
            let floor = content.get(kind)?;
            ensure!(
                floor.static_object
                    && floor.protects_ground
                    && !floor.blocks
                    && definitions.get(&kind).and_then(|d| d.native_service)
                        == Some(NativeService::Scenery),
                "{context}: invalid selected hazard-protecting chest platform {kind:#x}"
            );
            queue.push((kind, context.clone()));
        }
        queue.extend(
            chest
                .rows
                .iter()
                .flat_map(|r| &r.pool)
                .chain([&chest.landmark, &chest.container])
                .map(|kind| (*kind, context.clone())),
        );
    }
    while let Some((id, context)) = queue.pop() {
        if !reachable.insert(id) {
            continue;
        }
        let prop = content.get(id)?;
        if let Some(def) = definitions.get(&id) {
            queue.extend(def.dependencies.iter().map(|kind| {
                (
                    *kind,
                    format!("{} rule/loot dependency of {id:#x}", def.source),
                )
            }));
            for state in &def.states {
                for behavior in &state.behaviors {
                    for value in behavior.fields.values() {
                        if let Value::Map(index) = value {
                            queue.extend(maps[*index as usize].objects.iter().map(|kind| {
                                (
                                    *kind,
                                    format!(
                                        "{} map={} entity birth",
                                        behavior.source, maps[*index as usize].name
                                    ),
                                )
                            }));
                        }
                    }
                }
            }
        } else {
            bail!(
                "{context}: reachable identity {id:#x} {} has no explicit admission definition/disposition",
                prop.name
            );
        }
        if let Some(deps) = property_deps.get(&id) {
            queue.extend(deps.iter().map(|kind| {
                (
                    *kind,
                    format!(
                        "objects.xml selected property dependency owner={id:#x} {}",
                        prop.name
                    ),
                )
            }));
        }
    }
    let mut inputs: BTreeMap<_, _> = used
        .into_iter()
        .map(|p| {
            let h = hash(&bytes[&p]);
            (p, h)
        })
        .collect();
    for (file, h) in geometry_inputs {
        if let Some(old) = inputs.insert(file.clone(), h.clone()) {
            ensure!(
                old == h,
                "geometry dependency fingerprint disagreement {file}"
            );
        }
    }
    let fingerprint = hash(&serde_json::to_vec(&(1_u32, &inputs))?);
    let mut symbols = vec![String::new(); link.symbols.borrow().len()];
    for (text, index) in link.symbols.into_inner() {
        symbols[index as usize] = text;
    }
    let game = GameData {
        realm: realm_rules,
        fingerprint,
        inputs,
        symbols,
        string_lists,
        roots,
        definitions,
        maps,
        worlds,
        portal_routes,
        reachable,
        property_dependencies: property_deps,
        deployment_activations,
        landmark_chests,
        properties: content.objects.clone(),
    };
    validate_entity_edges(&game, &property_births)?;
    validate_world_routes(&game)?;
    validate_portal_contexts(&game)?;
    if let Some(rules) = &game.realm {
        realm::validate(&game, rules)
            .with_context(|| format!("{}:{} RealmRules", rules.source.file, rules.source.line))?;
    }
    ensure!(
        !world_ids.contains("realm") || game.realm.is_some(),
        "{MANIFEST}: Realm world requires RealmRules"
    );
    Ok(game)
}

fn native_service(name: &str, property: &Object, content: &Content) -> Result<NativeService> {
    use NativeService as N;
    let service = match name {
        "scenery" => N::Scenery,
        "companion" => N::Companion,
        "portal" => N::Portal,
        "merchant" => N::Merchant,
        "container" => N::Container,
        "inventory" => N::Inventory,
        "player" => N::Player,
        "projectile" => N::Projectile,
        "interaction" => N::Interaction,
        "trap" => N::Deployment(DeploymentKind::Trap),
        "decoy" => N::Deployment(DeploymentKind::Decoy),
        "spirit-bomb" => N::Deployment(DeploymentKind::SpiritBomb),
        _ => bail!("unknown nativeService {name}"),
    };
    let compatible = !property.enemy
        && match service {
            N::Scenery => matches!(
                property.class.as_str(),
                "GameObject"
                    | "Wall"
                    | "ConnectedWall"
                    | "CaveWall"
                    | "SpiderWeb"
                    | "Stalagmite"
                    | "Sign"
            ),
            N::Companion => {
                property.class == "Character"
                    && content.objects.values().any(|item| {
                        item.class == "Equipment"
                            && item.slot_type == 10
                            && item.activations == 1
                            && item
                                .companion
                                .as_ref()
                                .is_some_and(|companion| companion.object == property.name)
                    })
            }
            N::Portal => property.is_portal(),
            N::Merchant => matches!(property.class.as_str(), "Merchant" | "GuildMerchant"),
            N::Container => matches!(property.class.as_str(), "Container" | "ClosedVaultChest"),
            N::Inventory => matches!(property.class.as_str(), "Equipment" | "Dye"),
            N::Player => property.class == "Player",
            N::Projectile => property.class == "Projectile",
            N::Interaction => matches!(
                property.class.as_str(),
                "MoneyChanger"
                    | "CharacterChanger"
                    | "NameChanger"
                    | "GuildRegister"
                    | "GuildChronicle"
                    | "GuildBoard"
            ),
            N::Deployment(kind) => {
                property.class == "Character"
                    && content.objects.values().any(|item| {
                        item.class == "Equipment"
                            && item.activations == item.abilities.len()
                            && item
                                .abilities
                                .iter()
                                .any(|ability| DeploymentKind::of_ability(ability) == Some(kind))
                    })
            }
        };
    ensure!(
        compatible,
        "nativeService {name} incompatible with selected kind={:#x} {} class={} enemy={} or missing selected companion activation entitlement",
        property.kind,
        property.name,
        property.class,
        property.enemy
    );
    Ok(service)
}

fn validate_entity_edges(game: &GameData, property_births: &[(u16, u16, Source)]) -> Result<()> {
    let entity = |kind: u16, context: &str| -> Result<()> {
        let definition = game.definitions.get(&kind).with_context(|| {
            format!("{context}: kind={kind:#x} missing explicit entity admission definition")
        })?;
        game.entity_admission(kind, definition.admission_role()?, context)?;
        Ok(())
    };
    let mut maps = BTreeSet::new();
    for world in &game.worlds {
        maps.insert(world.map);
        for population in &world.populations {
            entity(population.object, &format!("world={} Population", world.id))?;
        }
    }
    for definition in game
        .definitions
        .values()
        .filter(|d| game.reachable.contains(&d.kind))
    {
        for behavior in definition.states.iter().flat_map(|state| &state.behaviors) {
            for (field, value) in &behavior.fields {
                let context = format!("{} birth={:?}/{field:?}", behavior.source, behavior.kind);
                match value {
                    Value::Map(index) => {
                        maps.insert(*index);
                    }
                    Value::Objects(kinds) if is_entity_birth_field(*field, behavior.kind) => {
                        for kind in kinds {
                            entity(*kind, &context)?;
                        }
                    }
                    _ => (),
                }
            }
        }
    }
    for index in maps {
        let map = &game.maps[index as usize];
        for kind in &map.objects {
            entity(
                *kind,
                &format!(
                    "{}:{} map={} entity placement",
                    map.source.file, map.source.line, map.name
                ),
            )?;
        }
    }
    for (item, edges) in &game.deployment_activations {
        if !game.reachable.contains(item) {
            continue;
        }
        for edge in edges {
            game.deployment_admission(
                edge.target,
                &game.properties[item],
                edge.activation,
                edge.kind,
                &format!("{} selected activation birth", edge.source),
            )?;
        }
    }
    for (owner, kind, source) in property_births
        .iter()
        .filter(|(owner, _, _)| game.reachable.contains(owner))
    {
        entity(
            *kind,
            &format!(
                "{}:{} property owner={owner:#x} Activate birth",
                source.file, source.line
            ),
        )?;
    }
    Ok(())
}
