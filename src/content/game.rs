//! Load-time server rules; client properties are never overlaid by server XML.
use super::{Content, Object};
use anyhow::Result;
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Arc,
};
mod compile;
pub mod geometry;
mod schema;
pub use geometry::MapBinding;
#[cfg(test)]
mod production_tests;
#[cfg(test)]
#[path = "game/tests/inputs.rs"]
pub(crate) mod test_inputs;
#[cfg(test)]
mod tests;
pub(crate) use compile::executor_reads;
pub use schema::{BehaviorKind, Field};

#[cfg(test)]
pub(crate) fn compile_fixture(
    content: &Content,
    inputs: BTreeMap<String, Vec<u8>>,
) -> Result<GameData> {
    compile::compile(content, inputs)
}

#[cfg(test)]
pub(crate) fn compile_fixture_at(
    content: &Content,
    inputs: BTreeMap<String, Vec<u8>>,
    root: &Path,
) -> Result<GameData> {
    compile::compile_at(content, inputs, Some(root))
}

pub type StateId = u32;
pub type LocalId = u32;
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Source {
    pub file: String,
    pub line: u32,
    pub offset: usize,
    pub object: String,
    pub path: String,
    pub embeds: Vec<String>,
}
impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{} object={} state={} embeds={:?}",
            self.file, self.line, self.object, self.path, self.embeds
        )
    }
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub enum Disposition {
    Active,
    Passive,
    Native,
}
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub enum NativeService {
    Scenery,
    Companion,
    Portal,
    Merchant,
    Container,
    Inventory,
    Player,
    Projectile,
    Interaction,
    Deployment(DeploymentKind),
}
impl NativeService {
    pub fn is_world_entity(self) -> bool {
        !matches!(
            self,
            Self::Inventory | Self::Player | Self::Projectile | Self::Deployment(_)
        )
    }
}
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum DeploymentKind {
    Trap,
    Decoy,
    SpiritBomb,
}
impl DeploymentKind {
    pub fn of_ability(ability: &super::Ability) -> Option<Self> {
        match ability {
            super::Ability::Trap { .. } => Some(Self::Trap),
            super::Ability::Decoy { .. } => Some(Self::Decoy),
            super::Ability::SpiritBomb => Some(Self::SpiritBomb),
            _ => None,
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct DeploymentActivation {
    pub activation: usize,
    pub kind: DeploymentKind,
    pub target: u16,
    pub source: Source,
}
fn deployment_definition(
    definitions: &BTreeMap<u16, Definition>,
    kind: DeploymentKind,
) -> Result<&Definition> {
    use anyhow::{Context, ensure};
    let mut matches = definitions
        .values()
        .filter(|d| d.native_service == Some(NativeService::Deployment(kind)));
    let definition = matches
        .next()
        .with_context(|| format!("missing {kind:?} deployment binding"))?;
    ensure!(
        matches.next().is_none(),
        "ambiguous {kind:?} deployment bindings"
    );
    Ok(definition)
}
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub enum AdmissionRole {
    Scripted,
    Native(NativeService),
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub enum Provenance {
    Recovered,
    Authored,
    Corrected,
}
#[derive(Clone, Debug, Serialize)]
pub enum Mode {
    Auto,
    Targeted,
    Flash,
}
#[derive(Clone, Debug, Serialize)]
pub enum TextPart {
    Literal(String),
    Killer,
    Player,
    Count,
    Hitpoints,
}
#[derive(Clone, Debug, Serialize)]
pub struct StringList {
    pub name: String,
    pub source: Source,
    pub categories: BTreeMap<String, Vec<Vec<TextPart>>>,
}
#[derive(Clone, Debug, Serialize)]
pub enum TerminalAnnouncements {
    /// Both terminal categories are absent in the parsed source, not missing mandatory prose.
    BirthOnly,
    Death {
        killed: Vec<Vec<TextPart>>,
        death: Vec<Vec<TextPart>>,
    },
}
#[derive(Clone, Debug, Serialize)]
pub struct AnnouncementRules {
    pub list: u32,
    pub source: Source,
    pub admission: Vec<Vec<TextPart>>,
    pub terminal: TerminalAnnouncements,
}
#[derive(Clone, Debug, Serialize)]
pub enum Condition {
    Selected(u8),
    StunImmune,
}
#[derive(Clone, Debug, Serialize)]
pub enum Value {
    Number(f64),
    Millis(u64),
    Radians(f64),
    Integer(u32),
    Bool(bool),
    TextChoices(Vec<Vec<TextPart>>),
    Limit(Option<u32>),
    NativeTerrain,
    Mode(Mode),
    Symbol(u32),
    StringList(u32),
    Colors(Vec<u32>),
    Nova,
    Objects(Vec<u16>),
    Grounds(Vec<u8>),
    Projectile(u8),
    Map(u32),
    Condition(Condition),
    Conditions(Vec<Condition>),
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub enum Emission {
    Projectile(u8),
    /// Authored lifecycle contract: births inherit actor world/encounter/parent ownership and live caps.
    Child(u16),
    /// Zero-shot Explode still retires the actor through its terminal lifecycle.
    Terminal,
}
#[derive(Clone, Debug, Serialize)]
pub struct Behavior {
    pub local: LocalId,
    pub kind: BehaviorKind,
    pub emission: Option<Emission>,
    pub announcement: Option<Arc<AnnouncementRules>>,
    pub bucket: Option<u32>,
    pub fields: BTreeMap<Field, Value>,
    pub source: Source,
}
#[derive(Clone, Debug, Serialize)]
pub struct Transition {
    /// Source list order and repeats survive linking; uniform entry choice is authored.
    pub targets: Vec<StateId>,
    /// Empty AND matches unconditionally; authored scheduling still permits only one winning transition per update.
    pub predicates: Vec<Predicate>,
    pub source: Source,
}
impl Transition {
    /// Only the winning transition may spend the actor's RNG; a singleton needs no draw.
    pub fn choose_destination(&self, won: bool, rng: &mut impl rand::Rng) -> Option<StateId> {
        if !won {
            return None;
        }
        assert!(
            !self.targets.is_empty(),
            "compiled transition has no destinations"
        );
        Some(
            self.targets[if self.targets.len() == 1 {
                0
            } else {
                rng.gen_range(0..self.targets.len())
            }],
        )
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct PlayerProximity {
    pub radius: f64,
    pub min_players: u32,
}
impl PlayerProximity {
    /// Authored count semantics; caller supplies only living, nondeparting owning-world players.
    pub fn matches_eligible_distances(
        &self,
        distances: impl IntoIterator<Item = f64>,
    ) -> Result<bool> {
        let mut count = 0usize;
        for distance in distances {
            anyhow::ensure!(
                distance.is_finite() && distance >= 0.,
                "invalid player distance {distance}"
            );
            count += usize::from(distance <= self.radius);
        }
        Ok(count >= self.min_players as usize)
    }
}
#[derive(Clone, Debug, Serialize)]
pub enum Predicate {
    After(u64),
    PlayerWithin(PlayerProximity),
    NoPlayerWithin(f64),
    HpBelow(f64),
    Random(f64),
    Exists(Vec<u16>),
    NoneExist(Vec<u16>),
    OnGround(Vec<u8>),
    Tag {
        symbol: u32,
        global: bool,
        present: bool,
    },
    PlayerSays(String),
}
#[derive(Clone, Debug, Serialize)]
pub struct Order {
    pub local: LocalId,
    /// None broadcasts to eligible listeners; never means an unresolved selector.
    pub targets: Option<Vec<u16>>,
    pub state: StateId,
    pub source: Source,
}
#[derive(Clone, Debug, Serialize)]
pub struct State {
    pub id: StateId,
    pub name: String,
    pub parent: Option<StateId>,
    pub children: Vec<StateId>,
    pub behaviors: Vec<Behavior>,
    pub transitions: Vec<Transition>,
    pub orders: Vec<Order>,
    pub source: Source,
}
#[derive(Clone, Debug, Serialize)]
pub enum LootOwnership {
    Public,
    Private,
}
#[derive(Clone, Debug, Serialize)]
pub enum LootCategory {
    Weapon,
    Ability,
    ClassSpecial,
    Armor,
    Ring,
    Potion,
}
#[derive(Clone, Debug, Serialize)]
pub struct RowOrigin {
    pub provenance: Provenance,
    pub origin: String,
    pub reason: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct LootRow {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub row_origin: Option<RowOrigin>,
    pub ownership: LootOwnership,
    pub category: Option<LootCategory>,
    pub tier: Option<u8>,
    pub pool: Vec<u16>,
    pub probability: f64,
    pub threshold: f64,
    pub min: u32,
    pub max: Option<u32>,
    pub source: Source,
}
/// Construction-time container contents owned by a landmark site, never its death loot or every container.
#[derive(Clone, Debug, Serialize)]
pub struct LandmarkChest {
    pub landmark: u16,
    pub container: u16,
    pub platform: Option<u16>,
    /// Inclusive success ceiling drawn before any row; later rows stop drawing once it is reached.
    pub ceiling: Option<(u32, u32)>,
    pub rows: Vec<LootRow>,
    pub provenance: Provenance,
    pub origin: String,
    pub reason: String,
    pub source: Source,
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct PopulationRule {
    pub terrain: Option<u8>,
    pub weight: Option<f64>,
    pub group: Option<super::SpawnGroup>,
    pub per_world_max: Option<u32>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Definition {
    pub kind: u16,
    pub disposition: Disposition,
    pub native_service: Option<NativeService>,
    pub provenance: Provenance,
    pub reason: String,
    pub origin: String,
    pub root: StateId,
    pub states: Vec<State>,
    pub loot: Vec<LootRow>,
    pub population: PopulationRule,
    pub xp_multiplier: f64,
    pub dependencies: BTreeSet<u16>,
    pub source: Source,
}
#[derive(Clone, Debug, Serialize)]
pub struct Population {
    pub object: u16,
    pub min: u32,
    pub max: u32,
}
macro_rules! progress_keys {
    ($($key:ident = $index:literal => $saved:literal, $world:literal;)+) => {
        #[derive(Clone, Copy, Debug, Serialize, serde::Deserialize, PartialEq, Eq)]
        pub enum ProgressKey { $(#[serde(rename = $saved)] $key,)+ }
        impl ProgressKey {
            pub const COUNT: usize = [$(Self::$key),+].len();
            pub fn index(self) -> usize { match self { $(Self::$key => $index,)+ } }
            pub fn world(self) -> &'static str { match self { $(Self::$key => $world,)+ } }
        }
    };
}
// Persisted progress must survive declaration and world catalogue reordering.
progress_keys! {
    PirateCave = 0 => "pirate_caves", "pirate-cave";
    SnakePit = 1 => "dungeons[0]", "snake-pit";
    SpriteWorld = 2 => "dungeons[1]", "sprite-world";
    UndeadLair = 3 => "dungeons[2]", "undead-lair";
    AbyssOfDemons = 4 => "abysses", "abyss-of-demons";
    OceanTrench = 5 => "trenches", "ocean-trench";
    SpiderDen = 6 => "spider_dens", "spider-den";
    MadLab = 7 => "mad_labs", "mad-lab";
    ForbiddenJungle = 8 => "jungles", "forbidden-jungle";
    ManorOfTheImmortals = 9 => "manors", "manor-of-the-immortals";
    Beachzone = 10 => "beaches", "beachzone";
    DavyJonesLocker = 11 => "davy_lockers", "davy-jones-locker";
    TombOfTheAncients = 12 => "tombs", "tomb-of-the-ancients";
}
pub type ExitKey = crate::dungeon::HistoricalDungeon;
#[derive(Clone, Debug, Serialize)]
pub enum Visibility {
    Full,
    Path,
    LineOfSight,
}
#[derive(Clone, Debug, Serialize)]
#[repr(i32)]
pub enum Background {
    None = 0,
    Stars = 1,
    Nexus = 2,
}
#[derive(Clone, Debug, Serialize)]
pub enum HelloNotification {
    ShowKeyUi,
}
#[derive(Clone, Debug, Serialize)]
pub struct WorldService<T> {
    pub value: T,
    pub provenance: Provenance,
    pub origin: String,
    pub reason: String,
    pub source: Source,
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct WorldServices {
    pub visibility: Option<WorldService<Visibility>>,
    pub difficulty: Option<WorldService<u8>>,
    pub background: Option<WorldService<Background>>,
    pub uses_breath: Option<WorldService<f64>>,
    pub hello_notification: Option<WorldService<HelloNotification>>,
    pub allow_teleport: Option<WorldService<bool>>,
    pub summon_limit: Option<WorldService<usize>>,
}
#[derive(Clone, Debug, Serialize)]
pub struct InstanceRule {
    pub entrance: u16,
    pub name: String,
    pub evidence: RowOrigin,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortalDestination {
    Nexus,
    OriginRealm,
    RandomRealm,
    NamedRealm,
    Instance(String),
    FreshKitchen,
    OwnedVault,
    GuildHall,
    Locked,
}
impl PortalDestination {
    pub fn can_publish(&self) -> bool {
        !matches!(
            self,
            Self::FreshKitchen | Self::OwnedVault | Self::GuildHall | Self::NamedRealm
        )
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct PortalRule {
    pub from: Option<String>,
    pub except: BTreeSet<String>,
    pub destination: PortalDestination,
    pub earned_exit: bool,
    pub evidence: RowOrigin,
}
#[derive(Clone, Debug, Serialize)]
pub struct CoilPlacement {
    pub rooms: usize,
    pub per_room: usize,
    pub boss_first: bool,
    pub evidence: RowOrigin,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClearRoute {
    Scripted,
    None,
    RealmExit(u16),
    Victory(u16),
}
#[derive(Clone, Debug, Serialize)]
pub struct ClearRule {
    pub boss: u16,
    pub route: ClearRoute,
    pub reveal: Option<u16>,
    pub retire: Option<u16>,
    pub evidence: RowOrigin,
}
#[derive(Clone, Debug, Serialize)]
pub struct World {
    pub id: String,
    pub map: u32,
    pub populations: Vec<Population>,
    pub services: WorldServices,
    /// Stable durable identity, never a compiler node index.
    pub clear_key: Option<ProgressKey>,
    pub exit_key: Option<ExitKey>,
    pub instance: Option<InstanceRule>,
    pub clear: Option<ClearRule>,
    pub coils: Option<CoilPlacement>,
    pub corner_placement: Option<CornerPlacement>,
    pub required_actors: Vec<u16>,
    pub unlock: Option<UnlockRule>,
}
#[derive(Clone, Debug, Serialize)]
pub struct UnlockRule {
    pub item: u16,
    pub locked: u16,
    pub opened: u16,
    pub evidence: RowOrigin,
}
#[derive(Clone, Debug, Serialize)]
pub struct CornerPlacement {
    pub kind: u16,
    pub evidence: RowOrigin,
}
#[derive(Clone, Debug, Serialize)]
pub struct RealmRules {
    pub density: Density,
    pub camps: BTreeMap<u16, Arc<CampRule>>,
    pub reproduction: BTreeMap<u16, Vec<ReproduceRule>>,
    pub sites: Vec<NamedSite>,
    pub source: Source,
}
#[derive(Clone, Debug, Serialize)]
pub struct Density {
    pub divisors: [u32; 12],
    pub numerator: u32,
    pub denominator: u32,
    pub evidence: RowOrigin,
}
#[derive(Clone, Debug, Serialize)]
pub struct CampRule {
    pub leader: u16,
    pub roles: Vec<CampRole>,
    pub evidence: RowOrigin,
}
impl CampRule {
    pub fn size(&self) -> usize {
        1 + self.roles.iter().map(|r| r.maximum).sum::<usize>()
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct CampRole {
    pub kind: u16,
    pub initial: usize,
    pub maximum: usize,
    pub interval: Option<u64>,
}
#[derive(Clone, Debug, Serialize)]
pub struct ReproduceRule {
    pub child: u16,
    pub radius: f32,
    pub maximum: usize,
    pub cooldown: u64,
    pub evidence: RowOrigin,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum SiteRole {
    Finite,
    Quest,
    Closure,
    Hero,
    Event,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SitePhase {
    Landmarks,
    AfterPopulation,
}
#[derive(Clone, Debug, Serialize)]
pub struct NamedSite {
    pub kind: u16,
    pub terrains: Vec<u8>,
    pub min: usize,
    pub max: usize,
    pub anchors_replace_roll: bool,
    pub role: SiteRole,
    pub phase: SitePhase,
    pub chance: Option<(u32, u32)>,
    pub round_robin: bool,
    pub anchor: Option<u16>,
    pub island: Option<Vec<IslandPlacement>>,
    pub interactive_tiles: BTreeMap<u8, u16>,
    pub evidence: RowOrigin,
}
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IslandOwnership {
    Defender,
    IndependentScenery,
}
#[derive(Clone, Debug, Serialize)]
pub struct IslandPlacement {
    pub kind: u16,
    pub x: i32,
    pub y: i32,
    pub ownership: IslandOwnership,
}
#[derive(Clone, Debug, Serialize)]
pub struct GameData {
    pub realm: Option<RealmRules>,
    pub fingerprint: String,
    pub inputs: BTreeMap<String, String>,
    pub symbols: Vec<String>,
    pub string_lists: Vec<StringList>,
    pub roots: BTreeSet<u16>,
    pub definitions: BTreeMap<u16, Definition>,
    pub maps: Vec<MapBinding>,
    pub worlds: Vec<World>,
    pub portal_routes: BTreeMap<u16, Vec<PortalRule>>,
    pub reachable: BTreeSet<u16>,
    pub property_dependencies: BTreeMap<u16, BTreeSet<u16>>,
    pub deployment_activations: BTreeMap<u16, Vec<DeploymentActivation>>,
    pub landmark_chests: BTreeMap<u16, LandmarkChest>,
    /// Exact selected-client definitions; server files have no property override syntax.
    pub properties: BTreeMap<u16, Object>,
}
impl Definition {
    pub fn admission_role(&self) -> Result<AdmissionRole> {
        match (&self.disposition, self.native_service) {
            (Disposition::Native, Some(service)) => Ok(AdmissionRole::Native(service)),
            (Disposition::Active | Disposition::Passive, None) => Ok(AdmissionRole::Scripted),
            _ => anyhow::bail!(
                "contradictory admission disposition/native service for {:#x}",
                self.kind
            ),
        }
    }
}
impl GameData {
    pub fn world_template(&self, id: &str) -> Option<&World> {
        self.worlds.iter().find(|world| world.id == id)
    }
    pub fn portal_route(&self, kind: u16, from: Option<&str>) -> Result<&PortalRule> {
        self.portal_routes
            .get(&kind)
            .and_then(|rules| {
                rules.iter().find(|rule| match &rule.from {
                    Some(scope) => Some(scope.as_str()) == from,
                    None => from.is_none_or(|scope| !rule.except.contains(scope)),
                })
            })
            .ok_or_else(|| anyhow::anyhow!("no linked portal route kind={kind:#x} from={from:?}"))
    }
    pub fn portal_instance(&self, kind: u16, from: Option<&str>) -> Result<&str> {
        let route = self.portal_route(kind, from)?;
        let PortalDestination::Instance(template) = &route.destination else {
            anyhow::bail!(
                "owned portal kind={kind:#x} from={from:?} requires an instance route, got {:?}",
                route.destination
            );
        };
        anyhow::ensure!(
            self.world_template(template)
                .and_then(|world| world.instance.as_ref())
                .is_some_and(|rule| rule.entrance == kind),
            "owned portal kind={kind:#x} from={from:?} does not bind a matching instance {template}"
        );
        Ok(template)
    }
    pub fn named_site(&self, kind: u16) -> Option<&NamedSite> {
        self.realm.as_ref()?.sites.iter().find(|s| s.kind == kind)
    }
    pub fn site_role(&self, kind: u16) -> Option<SiteRole> {
        self.named_site(kind).map(|s| s.role)
    }
    pub fn realm_rules(&self) -> Result<&RealmRules> {
        self.realm
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("game.xml: missing linked RealmRules"))
    }

    /// Resolve on the owning GameData at each ingress; a role is a request, not a transferable permit.
    pub fn admission(&self, kind: u16, role: AdmissionRole, context: &str) -> Result<&Definition> {
        use anyhow::{Context, ensure};
        (|| {
            let property = self.properties.get(&kind).context("unknown selected identity")?;
            let definition = self.definitions.get(&kind).context("missing explicit admission definition")?;
            ensure!(definition.kind == kind && definition.source.object == property.name,
                "selected admission identity mismatch");
            let declared = definition.admission_role()?;
            ensure!(declared == role,
                "admission role mismatch: requested {role:?}, declared {declared:?}; {}:{} object={} state={}",
                definition.source.file, definition.source.line, definition.source.object, definition.source.path);
            Ok(definition)
        })().with_context(|| format!("{context}: admission kind={kind:#x} role={role:?}"))
    }
    pub fn entity_admission(
        &self,
        kind: u16,
        role: AdmissionRole,
        context: &str,
    ) -> Result<&Definition> {
        let definition = self.admission(kind, role, context)?;
        anyhow::ensure!(
            !matches!(role, AdmissionRole::Native(service) if !service.is_world_entity()),
            "{context}: admission kind={kind:#x} role={role:?} is not a world entity for ordinary ingress (property-only or scoped deployment); {}:{} object={}",
            definition.source.file,
            definition.source.line,
            definition.source.object
        );
        Ok(definition)
    }
    /// Resolves the XML target binding only; this is not permission to deploy it.
    pub fn deployment_binding(&self, kind: DeploymentKind, context: &str) -> Result<&Definition> {
        use anyhow::Context;
        let definition =
            deployment_definition(&self.definitions, kind).with_context(|| context.to_owned())?;
        self.admission(
            definition.kind,
            AdmissionRole::Native(NativeService::Deployment(kind)),
            context,
        )
    }
    pub fn deployment_admission(
        &self,
        target: u16,
        item: &Object,
        activation: usize,
        kind: DeploymentKind,
        context: &str,
    ) -> Result<&Definition> {
        use anyhow::{Context, ensure};
        (|| {
            let selected = self.properties.get(&item.kind).context("unknown deployment item")?;
            ensure!(std::ptr::eq(selected, item), "deployment item is not the owning GameData selected item");
            self.admission(item.kind, AdmissionRole::Native(NativeService::Inventory), context)?;
            ensure!(item.class == "Equipment" && item.activations == item.abilities.len(), "deployment item activation metadata mismatch");
            ensure!(item.abilities.get(activation).and_then(DeploymentKind::of_ability) == Some(kind),
                "deployment activation/type mismatch");
            let definition = self.deployment_binding(kind, context)?;
            ensure!(definition.kind == target, "deployment target mismatch: requested {target:#x}, XML binding {:#x}", definition.kind);
            let edge = self.deployment_activations.get(&item.kind)
                .and_then(|edges| edges.iter().find(|edge| edge.activation == activation))
                .context("missing compiled selected deployment activation")?;
            ensure!(edge.kind == kind && edge.target == target,
                "selected deployment activation/target mismatch; {}:{} object={} state={}",
                edge.source.file, edge.source.line, edge.source.object, edge.source.path);
            Ok(definition)
        })().with_context(|| format!("{context}: deployment={kind:?} item={:#x} activation={activation} target={target:#x}", item.kind))
    }
    pub fn load(data: &Path, content: &Content) -> Result<Self> {
        compile::load(data, content)
    }
    /// Bot advisory only: some compiled MetamorphoseAtDeath row of `kind` can birth `child` (prob > 0).
    pub fn births_at_death(&self, kind: u16, child: u16) -> bool {
        self.definitions.get(&kind).is_some_and(|d| {
            d.states.iter().flat_map(|s| &s.behaviors).any(|b| {
                b.kind == BehaviorKind::MetamorphoseAtDeath
                    && matches!(b.fields.get(&Field::ChildId), Some(Value::Objects(ids)) if ids.contains(&child))
                    && !matches!(b.fields.get(&Field::Prob), Some(Value::Number(p)) if *p <= 0.)
            })
        })
    }
}
