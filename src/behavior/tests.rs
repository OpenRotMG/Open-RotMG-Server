use super::*;
use crate::assert_matches;
use crate::content::{
    Content,
    game::{
        self, AdmissionRole, Disposition, Emission, GameData, Mode, NativeService, PlayerProximity,
        Transition,
    },
};
use rand::{RngCore, rngs::mock::StepRng};
use std::{cell::RefCell, path::Path, sync::LazyLock};

static CONTENT: LazyLock<Content> = LazyLock::new(|| Content::load(Path::new("data")).unwrap());
static PRODUCTION: LazyLock<GameData> =
    LazyLock::new(|| GameData::load(Path::new("data"), &CONTENT).unwrap());
fn authored(name: &str, id: u16, body: &str) -> String {
    format!(
        r#"<Object type="{id}" id="{name}" disposition="active" provenance="authored" reason="executor regression" origin="independent fixture">{body}</Object>"#
    )
}
const FIXTURE_BLADE: &str = r#"<Object type="6" id="Blade" disposition="native" nativeService="projectile" provenance="authored" origin="selected Pirate projectile property" reason="bounded executor fixture dependency"/>"#;
fn fixture_rules(rules: &str) -> Vec<u8> {
    format!("<Objects>{rules}{FIXTURE_BLADE}</Objects>").into_bytes()
}
fn compile_with(content: &Content, rules: String, root: &str) -> GameData {
    game::compile_fixture(content, crate::content::game::test_inputs::selected([
        ("game/123.5.1/game.xml", format!(r#"<Game schema="1"><Include file="rules.xml"/><Root object="{root}"/><Geometry file="geometry.xml"/></Game>"#).into_bytes()),
        ("game/123.5.1/rules.xml", fixture_rules(&rules)),
        ("game/123.5.1/geometry.xml", br#"<Geometry schema="1"><Template id="Contract Map" provenance="authored" origin="executor test" reason="service routing fixture" width="2" height="2" rotation="fixed" rooms="1"><CharMap char="." ground="Grass"/><Pattern><Row value=".."/><Row value=".."/></Pattern><Spawn x="0.5" y="0.5"/></Template></Geometry>"#.to_vec()),
    ])).unwrap()
}
fn compile(body: &str) -> Arc<Definition> {
    let body = if body.contains("<Behavior") {
        body.to_owned()
    } else {
        format!("<Behavior>RemainVisible</Behavior>{body}")
    };
    Arc::new(
        compile_with(
            &CONTENT,
            authored("Pirate", 0x600, &body)
                + &authored("Piratess", 0x601, "<Behavior>RemainVisible</Behavior>"),
            "Pirate",
        )
        .definitions
        .remove(&0x600)
        .unwrap(),
    )
}
fn identity(actor: i32) -> Identity {
    Identity {
        world: 1,
        actor,
        parent: Some(8),
        encounter: Some(9),
    }
}
fn actor(def: Arc<Definition>) -> Actor {
    Actor::new(def, identity(10), 12345).unwrap()
}
#[derive(Debug, Clone)]
struct Event {
    time: u64,
    scope: Scope,
    text: String,
}
#[derive(Default)]
struct Recording {
    time: u64,
    events: Vec<Event>,
    offers: Vec<Offer>,
    blocked: Vec<K>,
    ready_calls: RefCell<Vec<(Service, K, Step)>>,
    fail_ready: bool,
    players: Vec<f64>,
    hp: f64,
    exists: bool,
    ground: u8,
    says: bool,
    global: BTreeSet<(u64, u32)>,
    size: f64,
    fail: bool,
    ongoing: bool,
    fail_terminal: bool,
    remove: bool,
    reject: Vec<K>,
    draw_native: bool,
    native_attempts: Vec<(K, Step, Option<u64>)>,
    child_requests: Vec<(u64, Vec<u16>)>,
    deadlines: Vec<(Deadline, u64)>,
    say_choices: Vec<(u64, usize)>,
    terminal_attempts: Vec<String>,
    admission_attempts: Vec<Announcement>,
    admission_claims: BTreeMap<AnnouncementKey, String>,
    fail_admission: bool,
    fail_admission_after_claim: bool,
    fail_terminal_after_claim: bool,
    terminal_claims: BTreeSet<Identity>,
    terminal_plans: Vec<TerminalPlan>,
    terminal_announcements: Vec<Announcement>,
}
impl Recording {
    fn new() -> Self {
        Self {
            size: 100.,
            hp: 1.,
            ..Self::default()
        }
    }
    fn record(&mut self, scope: Scope, text: String) {
        self.events.push(Event {
            time: self.time,
            scope,
            text,
        });
    }
    fn actions(&self) -> Vec<(u64, String)> {
        self.events
            .iter()
            .filter(|e| e.text.starts_with("apply:"))
            .map(|e| (e.time, e.text.clone()))
            .collect()
    }
    fn labels(&self, prefix: &str) -> Vec<(u64, String)> {
        self.events
            .iter()
            .filter(|e| e.text.starts_with(prefix))
            .map(|e| (e.time, e.text.clone()))
            .collect()
    }
}
impl World for Recording {
    fn players(&self, _: Scope) -> Result<Vec<f64>> {
        Ok(self.players.clone())
    }
    fn hp_fraction(&self, _: Scope) -> Result<f64> {
        Ok(self.hp)
    }
    fn exists(&self, _: Scope, _: &[u16]) -> Result<bool> {
        Ok(self.exists)
    }
    fn ground(&self, _: Scope) -> Result<u8> {
        Ok(self.ground)
    }
    fn global_tag(&self, s: Scope, symbol: u32) -> Result<bool> {
        Ok(self.global.contains(&(s.identity.world, symbol)))
    }
    fn player_says(&self, _: Scope, _: &str) -> Result<bool> {
        Ok(self.says)
    }
    fn orders(&self, _: Scope) -> Result<Vec<Offer>> {
        Ok(self.offers.clone())
    }
    fn size(&self, _: Scope) -> Result<f64> {
        Ok(self.size)
    }
    fn ready(&self, service: Service, b: &Behavior, step: Step) -> Result<bool> {
        self.ready_calls.borrow_mut().push((service, b.kind, step));
        ensure!(!self.fail_ready, "native readiness rejected: {:?}", b.kind);
        Ok(!self.blocked.contains(&b.kind))
    }
    fn advance(&mut self, b: &Behavior, step: Step) -> Result<bool> {
        if self.ongoing && !self.blocked.contains(&b.kind) {
            self.record(
                step.token.scope,
                format!("advance:{:?}:{}", b.kind, step.delta_ms),
            );
            Ok(true)
        } else {
            Ok(false)
        }
    }
    fn apply(
        &mut self,
        _: Service,
        b: &Behavior,
        step: Step,
        rng: &mut StdRng,
    ) -> Result<ActionOutcome> {
        let draw = self.draw_native.then(|| rng.next_u64());
        self.native_attempts.push((b.kind, step, draw));
        if self.reject.contains(&b.kind) {
            return Ok(ActionOutcome::Blocked);
        }
        ensure!(!self.fail, "native service unsupported: {:?}", b.kind);
        if let Some(Value::Objects(ids)) = b
            .fields
            .get(&F::ChildIds)
            .or_else(|| b.fields.get(&F::ChildId))
        {
            self.child_requests.push((self.time, ids.clone()));
        }
        if b.kind == K::Say {
            self.say_choices.push((
                self.time,
                step.text_choice.context("Say missing chosen alternative")?,
            ));
        }
        self.record(step.token.scope, format!("apply:{:?}:{}", b.kind, b.local));
        Ok(if self.remove {
            ActionOutcome::Removed
        } else {
            ActionOutcome::Applied
        })
    }
    fn announce(&mut self, scope: Scope, announcement: &Announcement) -> Result<()> {
        assert_eq!(scope, announcement.key.owner.scope);
        assert_eq!(announcement.key.category, AnnouncementCategory::New);
        self.admission_attempts.push(announcement.clone());
        ensure!(!self.fail_admission, "admission rejected before claim");
        if let std::collections::btree_map::Entry::Vacant(e) =
            self.admission_claims.entry(announcement.key)
        {
            e.insert(announcement.text.clone());
            self.record(scope, format!("announcement:New:{}", announcement.text));
        }
        ensure!(
            !self.fail_admission_after_claim,
            "admission error after claim"
        );
        Ok(())
    }
    fn terminal(&mut self, scope: Scope, plan: &TerminalPlan) -> Result<()> {
        self.terminal_plans.push(plan.clone());
        if self.terminal_claims.contains(&scope.identity) {
            return Ok(());
        }
        self.terminal_attempts.push(format!("{plan:?}"));
        ensure!(!self.fail_terminal, "atomic terminal rejected");
        for hook in &plan.hooks {
            assert_eq!(hook.owner.scope.identity, scope.identity);
            self.record(scope, format!("death:{:?}", hook.behavior.kind));
        }
        self.record(
            scope,
            format!(
                "terminal:{:?}:{}:{}",
                plan.reason, plan.rewards.loot, plan.rewards.xp
            ),
        );
        for announcement in &plan.announcements {
            assert_eq!(announcement.key.owner.scope.identity, scope.identity);
            assert_ne!(announcement.key.category, AnnouncementCategory::New);
            self.terminal_announcements.push(announcement.clone());
            self.record(
                scope,
                format!(
                    "announcement:{:?}:{}",
                    announcement.key.category, announcement.text
                ),
            );
        }
        self.terminal_claims.insert(scope.identity);
        ensure!(
            !self.fail_terminal_after_claim,
            "terminal error after claim"
        );
        Ok(())
    }
    fn effect(&mut self, scope: Scope, effect: Effect<'_>) -> Result<()> {
        let text = match effect {
            Effect::Enter(s) => format!("enter:{s}"),
            Effect::Exit(s) => format!("exit:{s}"),
            Effect::ForgetLocal(t) => {
                self.deadlines.retain(|(deadline, _)| deadline.owner != t);
                format!("forget:{}", t.local)
            }
            Effect::ScheduleLifecycle { deadline, delay_ms } => {
                self.deadlines
                    .push((deadline, self.time.checked_add(delay_ms).unwrap()));
                format!("schedule-lifecycle:{}:{delay_ms}", deadline.owner.local)
            }
            Effect::Condition {
                owner,
                condition,
                enabled,
            } => format!("condition:{}:{condition:?}:{enabled}", owner.local),
            Effect::GlobalTag {
                symbol,
                present,
                notify,
            } => {
                if notify {
                    self.record(scope, format!("notification:{symbol}"));
                }
                if present {
                    self.global.insert((scope.identity.world, symbol));
                } else {
                    self.global.remove(&(scope.identity.world, symbol));
                }
                format!("global:{symbol}:{present}")
            }
            Effect::PublishOrder { local, .. } => format!("publish:{local}"),
            Effect::WithdrawOrder(local) => format!("withdraw:{local}"),
            Effect::ClearAltTexture => "clear-alt-texture".into(),
            Effect::Size(size) => {
                self.size = size;
                format!("size:{size:.4}")
            }
        };
        self.record(scope, text);
        Ok(())
    }
}
fn pump_deadlines(a: &mut Actor, w: &mut Recording) -> Result<()> {
    while let Some(index) = w.deadlines.iter().position(|(deadline, due)| {
        deadline.owner.scope.identity == a.scope.identity && *due <= w.time
    }) {
        let (deadline, _) = w.deadlines.remove(index);
        a.expire_lifecycle(deadline, w)?;
    }
    Ok(())
}
fn update(a: &mut Actor, w: &mut Recording, at: u64, frame: Frame) -> Result<()> {
    w.time = at;
    pump_deadlines(a, w)?;
    a.tick(frame, w)?;
    pump_deadlines(a, w)
}
fn tick(a: &mut Actor, w: &mut Recording, at: u64, delta_ms: u64) {
    update(
        a,
        w,
        at,
        Frame {
            delta_ms,
            ..Frame::default()
        },
    )
    .unwrap();
}
fn receipt(name: &str, value: serde_json::Value) {
    if let Some(dir) = std::env::var_os("QA_RESULTS_DIR") {
        std::fs::write(
            Path::new(&dir).join(name),
            serde_json::to_vec_pretty(&value).unwrap(),
        )
        .unwrap();
    }
}
#[test]
fn xml_runtime_independent_phase1_exact_hierarchy_trace() {
    let mut content = CONTENT.clone();
    let mut main = content.objects[&0x600].clone();
    main.kind = 0xfffe;
    main.name = "Contract Actor".into();
    main.projectiles.insert(1, main.projectiles[&0].clone());
    let mut follower = main.clone();
    follower.kind = 0xfffd;
    follower.name = "Contract Follower".into();
    follower.projectiles.clear();
    content.objects.insert(main.kind, main);
    content.objects.insert(follower.kind, follower);
    let xml = include_str!("../../tests/fixtures/xml_contract/semantics.xml");
    let doc = roxmltree::Document::parse(xml).unwrap();
    let mut rules = String::new();
    for object in doc.root_element().children().filter(|n| n.is_element()) {
        let body: String = object
            .children()
            .filter(|n| {
                n.is_element()
                    && [
                        "Behavior",
                        "State",
                        "Transition",
                        "Order",
                        "DefineEmbed",
                        "Embed",
                    ]
                    .contains(&n.tag_name().name())
            })
            .map(|n| &xml[n.range()])
            .collect();
        rules.push_str(&authored(
            object.attribute("id").unwrap(),
            u16::from_str_radix(
                object
                    .attribute("type")
                    .unwrap()
                    .strip_prefix("0x")
                    .unwrap(),
                16,
            )
            .unwrap(),
            &body,
        ));
    }
    let mut game = compile_with(&content, rules, "Contract Actor");
    let linked = &game.definitions[&0xfffe];
    let first = linked.states.iter().find(|s| s.name == "First").unwrap();
    assert_eq!(first.behaviors.len(), 2);
    assert_ne!(first.behaviors[0].local, first.behaviors[1].local);
    assert_matches!(first.behaviors[0].fields[&F::Offset], Value::Millis(0));
    assert_matches!(first.behaviors[1].fields[&F::Offset], Value::Millis(100));
    assert!(
        first
            .behaviors
            .iter()
            .all(|b| b.source.embeds[0].starts_with("Pulse@"))
    );
    assert_eq!(
        linked.states[first.transitions[0].targets[0] as usize].name,
        "Second"
    );
    let active = linked.states.iter().find(|s| s.name == "Active").unwrap();
    assert_eq!(active.orders[0].targets, Some(vec![0xfffd]));
    assert!(game.reachable.contains(&0xfffd));
    assert_eq!(
        linked.states[0]
            .children
            .iter()
            .map(|i| linked.states[*i as usize].name.as_str())
            .collect::<Vec<_>>(),
        ["Active", "Done"]
    );

    let def = Arc::new(game.definitions.remove(&0xfffe).unwrap());
    let mut a = actor(def.clone());
    let mut w = Recording::new();
    for at in (0..=600).step_by(50) {
        tick(&mut a, &mut w, at, 50);
    }
    let shots: Vec<_> = w
        .events
        .iter()
        .filter(|e| e.text.starts_with("apply:Shoot"))
        .map(|e| {
            let local: u32 = e.text.rsplit(':').next().unwrap().parse().unwrap();
            let b = def
                .states
                .iter()
                .flat_map(|s| &s.behaviors)
                .find(|b| b.local == local)
                .unwrap();
            let Emission::Projectile(projectile) = b.emission.as_ref().unwrap() else {
                panic!()
            };
            (e.time, local, *projectile)
        })
        .collect();
    let first = def.states.iter().find(|s| s.name == "First").unwrap();
    let second = def.states.iter().find(|s| s.name == "Second").unwrap();
    assert_eq!(
        shots,
        vec![
            (0, first.behaviors[0].local, 0),
            (100, first.behaviors[1].local, 0),
            (200, first.behaviors[0].local, 0),
            (250, second.behaviors[0].local, 1),
            (350, second.behaviors[0].local, 1)
        ]
    );
    let entries: Vec<_> = w
        .labels("enter:")
        .into_iter()
        .map(|(t, s)| (t, def.states[s[6..].parse::<usize>().unwrap()].name.clone()))
        .collect();
    assert_eq!(
        entries,
        vec![
            (0, "@root".into()),
            (0, "Active".into()),
            (0, "First".into()),
            (200, "Second".into()),
            (400, "Done".into())
        ]
    );
    let exits: Vec<_> = w
        .labels("exit:")
        .into_iter()
        .map(|(t, s)| (t, def.states[s[5..].parse::<usize>().unwrap()].name.clone()))
        .collect();
    assert_eq!(
        exits,
        vec![
            (200, "First".into()),
            (400, "Second".into()),
            (400, "Active".into()),
            (600, "Done".into()),
            (600, "@root".into())
        ]
    );
    assert_eq!(
        w.labels("condition:")
            .iter()
            .map(|(t, s)| (*t, s.ends_with("true")))
            .collect::<Vec<_>>(),
        vec![(0, true), (400, false)]
    );
    assert!(
        w.events
            .iter()
            .position(|e| e.text.starts_with("condition:"))
            .unwrap()
            < w.events
                .iter()
                .position(|e| e.text.starts_with("apply:Shoot"))
                .unwrap()
    );
    assert_eq!(
        w.events
            .iter()
            .filter(|e| e.text.starts_with("apply:Wander"))
            .map(|e| e.time)
            .collect::<Vec<_>>(),
        (0..600).step_by(50).collect::<Vec<_>>()
    );
    assert_eq!(
        w.labels("terminal:"),
        vec![(600, "terminal:Decay:false:false".into())]
    );
    assert!(a.retired());
    let oracle: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/xml_contract/expectations.json"
    ))
    .unwrap();
    assert_eq!(oracle["state"]["retained_ancestor_until_ms"], 400);
    assert_eq!(
        oracle["state"]["root_movement_ticks_ms"],
        serde_json::json!((0..600).step_by(50).collect::<Vec<_>>())
    );
    receipt(
        "xml-runtime-hierarchy.json",
        serde_json::json!({"qualification":"Observed executor/mock native-service trace, not gameplay or historical acceptance", "seed":12345,"shots":shots,"entries":entries,"exits":exits,"events":w.events.iter().map(|e| format!("{}:{}",e.time,e.text)).collect::<Vec<_>>() }),
    );
}
#[test]
fn xml_runtime_buckets_child_first_failure_cooldown_stun_and_no_catchup() {
    let def = compile(
        r#"<Behavior bucket="movement">Wander</Behavior><Behavior type="auto" cooldown=".1">Shoot</Behavior><State id="A"><Behavior bucket="movement" cooldown=".1">Charge</Behavior></State>"#,
    );
    let mut a = actor(def.clone());
    let mut w = Recording::new();
    tick(&mut a, &mut w, 0, 0);
    tick(&mut a, &mut w, 50, 50);
    w.blocked.push(K::Charge);
    tick(&mut a, &mut w, 100, 50);
    w.time = 150;
    a.tick(
        Frame {
            delta_ms: 50,
            stunned: true,
            ..Frame::default()
        },
        &mut w,
    )
    .unwrap();
    w.blocked.clear();
    tick(&mut a, &mut w, 1000, 850);
    let kinds: Vec<_> = w
        .actions()
        .iter()
        .map(|(t, s)| (*t, s.split(':').nth(1).unwrap().to_string()))
        .collect();
    assert_eq!(
        kinds,
        vec![
            (0, "Charge".into()),
            (0, "Shoot".into()),
            (50, "Wander".into()),
            (100, "Wander".into()),
            (100, "Shoot".into()),
            (150, "Wander".into()),
            (1000, "Charge".into()),
            (1000, "Shoot".into())
        ]
    );
    assert_eq!(a.active_ms(), 1000);
}
#[test]
fn xml_runtime_stasis_dormancy_freeze_offsets_state_age_and_failed_attack_rng() {
    let def = compile(
        r#"<State id="A"><Behavior type="auto" cooldown=".1" offset=".1" cooldownJitter="true">Shoot</Behavior><Transition afterTime=".2">B</Transition></State><State id="B"/>"#,
    );
    let local = def.states[1].behaviors[0].local;
    let mut a = actor(def);
    let mut w = Recording::new();
    tick(&mut a, &mut w, 0, 0);
    let mut before = a.locals[&local].rng.clone();
    for frame in [
        Frame {
            delta_ms: 1000,
            stasis: true,
            ..Frame::default()
        },
        Frame {
            delta_ms: 1000,
            dormant: true,
            ..Frame::default()
        },
    ] {
        a.tick(frame, &mut w).unwrap();
    }
    assert_eq!(a.active_ms(), 0);
    w.blocked.push(K::Shoot);
    tick(&mut a, &mut w, 50, 50);
    tick(&mut a, &mut w, 100, 50);
    w.time = 150;
    a.tick(
        Frame {
            delta_ms: 50,
            stunned: true,
            ..Frame::default()
        },
        &mut w,
    )
    .unwrap();
    assert_eq!(a.locals[&local].rng.clone().next_u64(), before.next_u64());
    w.blocked.clear();
    tick(&mut a, &mut w, 200, 50);
    assert_eq!(w.actions().len(), 1);
    assert_eq!(w.actions()[0].0, 200);
    assert_eq!(
        a.definition.states[*a.path().last().unwrap() as usize].name,
        "B"
    );
}
#[test]
fn xml_runtime_local_reset_and_overlapping_conditions_clean_deepest_first() {
    let def = compile(
        r#"<Behavior effect="Invulnerable">ConditionEffect</Behavior><State id="A"><Behavior effect="Invulnerable">ConditionEffect</Behavior><Transition afterTime=".1">B</Transition></State><State id="B"><Transition afterTime=".1">A</Transition></State>"#,
    );
    let root_local = def.states[0].behaviors[0].local;
    let child_local = def.states[1].behaviors[0].local;
    let mut a = actor(def);
    let mut w = Recording::new();
    tick(&mut a, &mut w, 0, 0);
    tick(&mut a, &mut w, 100, 100);
    tick(&mut a, &mut w, 200, 100);
    a.cleanup(&mut w).unwrap();
    assert_eq!(
        w.labels("condition:"),
        vec![
            (0, format!("condition:{root_local}:Selected(25):true")),
            (0, format!("condition:{child_local}:Selected(25):true")),
            (100, format!("condition:{child_local}:Selected(25):false")),
            (200, format!("condition:{child_local}:Selected(25):true")),
            (200, format!("condition:{child_local}:Selected(25):false")),
            (200, format!("condition:{root_local}:Selected(25):false"))
        ]
    );
}
#[test]
fn xml_runtime_timed_conditions_and_size_per_200ms_fractional_clamp() {
    let def = compile(
        r#"<Behavior effect="Invulnerable" duration=".1" cooldown=".2">ConditionEffect</Behavior><Behavior minSize="99" maxSize="100.12" rate=".2">ChangeSize</Behavior>"#,
    );
    let mut a = actor(def);
    let mut w = Recording::new();
    for at in [0, 50, 100, 150, 200] {
        tick(&mut a, &mut w, at, 50);
    }
    assert_eq!(
        w.labels("size:"),
        vec![
            (0, "size:100.0000".into()),
            (50, "size:100.0500".into()),
            (100, "size:100.1000".into()),
            (150, "size:100.1200".into()),
            (200, "size:100.1200".into())
        ]
    );
    assert_eq!(
        w.labels("condition:")
            .iter()
            .map(|(t, s)| (*t, s.ends_with("true")))
            .collect::<Vec<_>>(),
        vec![(0, true), (100, false), (200, true)]
    );
}
#[test]
fn xml_runtime_predicates_and_and_or_boundaries_and_ancestor_precedence() {
    let def = compile(
        r#"<State id="A"><State id="Child"><Transition afterTime="0">Other</Transition></State><State id="Other"/><Transition afterTime=".1" playerWithin="2">B</Transition><Transition afterTime=".1" hitpointsLessThan=".5">C</Transition></State><State id="B"/><State id="C"/>"#,
    );
    let mut a = actor(def);
    let mut w = Recording::new();
    w.players = vec![2.];
    w.hp = 0.5;
    tick(&mut a, &mut w, 0, 0);
    assert_eq!(
        a.definition.states[*a.path().last().unwrap() as usize].name,
        "Other"
    );
    tick(&mut a, &mut w, 100, 100);
    assert_eq!(
        a.definition.states[*a.path().last().unwrap() as usize].name,
        "B"
    );
    let scope = a.scope;
    assert!(
        a.matches(
            &Predicate::PlayerWithin(PlayerProximity {
                radius: 2.,
                min_players: 1
            }),
            0,
            &w
        )
        .unwrap()
    );
    assert!(!a.matches(&Predicate::NoPlayerWithin(2.), 0, &w).unwrap());
    assert!(!a.matches(&Predicate::HpBelow(0.5), 0, &w).unwrap());
    w.hp = 0.499;
    assert!(a.matches(&Predicate::HpBelow(0.5), 0, &w).unwrap());
    w.exists = true;
    assert!(a.matches(&Predicate::Exists(vec![0x600]), 0, &w).unwrap());
    assert!(
        !a.matches(&Predicate::NoneExist(vec![0x600]), 0, &w)
            .unwrap()
    );
    w.ground = 4;
    assert!(a.matches(&Predicate::OnGround(vec![4]), 0, &w).unwrap());
    w.says = true;
    assert!(
        a.matches(&Predicate::PlayerSays("hello".into()), 0, &w)
            .unwrap()
    );
    w.players = vec![f64::NAN];
    assert!(a.matches(&Predicate::NoPlayerWithin(2.), 0, &w).is_err());
    assert_eq!(a.scope, scope);
}
#[test]
fn xml_runtime_zero_time_cycles_only_one_transition_per_update() {
    let mut a = actor(compile(
        r#"<State id="A"><Transition>B</Transition></State><State id="B"><Transition>A</Transition></State>"#,
    ));
    let mut w = Recording::new();
    for at in [0, 50, 100] {
        tick(&mut a, &mut w, at, 50);
    }
    assert_eq!(
        w.labels("enter:")
            .iter()
            .map(|(t, s)| (*t, s.as_str()))
            .collect::<Vec<_>>(),
        vec![
            (0, "enter:0"),
            (0, "enter:1"),
            (0, "enter:2"),
            (50, "enter:1"),
            (100, "enter:2")
        ]
    );
}
#[test]
fn xml_runtime_seeded_random_order_not_normalized_and_no_losing_destination_draw() {
    let def = compile(
        r#"<State id="A"><Transition random=".5">B,C,B</Transition><Transition>C</Transition></State><State id="B"/><State id="C"/>"#,
    );
    let mut observed = vec![];
    for seed in [0, 1, 2, 3, 4, 5, 6, 7] {
        let mut a = Actor::new(def.clone(), identity(10), seed).unwrap();
        let mut w = Recording::new();
        tick(&mut a, &mut w, 0, 0);
        observed.push(
            a.definition.states[*a.path.last().unwrap() as usize]
                .name
                .clone(),
        );
    }
    // Raw rand0.8.8 samples were obtained without invoking this compiler/executor:
    // u: .731,.974,.297,.095,.533,.342,.335,.030; subsequent indices: 2,2,0,1,1,0,2,0.
    assert_eq!(observed, vec!["C", "C", "B", "C", "C", "B", "B", "B"]);
    let source = def.source.clone();
    let transition = Transition {
        targets: vec![1, 2],
        predicates: vec![],
        source,
    };
    let mut rng = StepRng::new(0, 0);
    assert_eq!(transition.choose_destination(false, &mut rng), None);
    assert_eq!(rng.next_u64(), 0);
    let mut a = actor(def);
    let mut w = Recording::new();
    let mut before = a.rng.clone();
    assert!(!a.matches(&Predicate::Random(0.), 0, &w).unwrap());
    assert!(a.matches(&Predicate::Random(1.), 0, &w).unwrap());
    assert_eq!(a.rng.next_u64(), before.next_u64());
    w.players.clear();
}
#[test]
fn xml_runtime_actor_local_and_global_tags_world_isolation() {
    let def = compile(
        r#"<State id="A"><Behavior tagName="local">SetTag</Behavior><Behavior tagName="world" sendNotification="true">SetGlobalTag</Behavior><Transition afterTime=".1">B</Transition></State><State id="B"><Behavior tagName="local">ClearTag</Behavior><Behavior tagName="world">ClearGlobalTag</Behavior></State>"#,
    );
    let mut a = actor(def.clone());
    let mut other = Actor::new(
        def,
        Identity {
            world: 2,
            ..identity(20)
        },
        9,
    )
    .unwrap();
    let mut w = Recording::new();
    tick(&mut a, &mut w, 0, 0);
    assert!(a.has_tag(0));
    assert!(!other.has_tag(0));
    assert!(
        a.matches(
            &Predicate::Tag {
                symbol: 1,
                global: true,
                present: true
            },
            0,
            &w
        )
        .unwrap()
    );
    assert!(
        !other
            .matches(
                &Predicate::Tag {
                    symbol: 1,
                    global: true,
                    present: true
                },
                0,
                &w
            )
            .unwrap()
    );
    tick(&mut a, &mut w, 100, 100);
    assert!(a.has_tag(0));
    tick(&mut a, &mut w, 150, 50);
    assert!(!a.has_tag(0));
    assert!(!w.global.contains(&(1, 1)));
    assert_eq!(
        w.labels("notification:"),
        vec![(0, "notification:1".into()), (100, "notification:1".into())]
    );
}
fn offer(issuer: &Arc<Definition>, order: &crate::content::game::Order) -> Offer {
    Offer {
        issuer: identity(20),
        definition: issuer.clone(),
        local: order.local,
        state: order.state,
        generation: 1,
        targets: order.targets.clone(),
        distance: 1.,
    }
}
fn ordered_fixture() -> (Arc<Definition>, Arc<Definition>) {
    let rules = authored(
        "Pirate",
        0x600,
        r#"<State id="A"><Order targetIds="Piratess"><Behavior bucket="movement">Protect</Behavior><State id="First"><Behavior effect="Invulnerable">ConditionEffect</Behavior><Transition afterTime=".1">Second</Transition></State><State id="Second"><Behavior tagName="from-order">SetTag</Behavior></State></Order></State>"#,
    ) + &authored(
        "Piratess",
        0x601,
        r#"<Behavior bucket="movement" listenIds="Pirate" acquireRange="8">FollowOrders</Behavior><Behavior bucket="movement">Wander</Behavior>"#,
    );
    let mut game = compile_with(&CONTENT, rules, "Pirate");
    (
        Arc::new(game.definitions.remove(&0x600).unwrap()),
        Arc::new(game.definitions.remove(&0x601).unwrap()),
    )
}
#[test]
fn xml_runtime_orders_nearest_id_filter_independent_listeners_and_invalidation() {
    let (issuer, follower) = ordered_fixture();
    let order = issuer.states[1].orders[0].clone();
    let mut a = Actor::new(follower.clone(), identity(10), 12).unwrap();
    let mut b = Actor::new(follower, identity(11), 13).unwrap();
    let mut w = Recording::new();
    w.offers = vec![
        Offer {
            issuer: identity(30),
            definition: issuer.clone(),
            local: order.local,
            state: order.state,
            generation: 1,
            targets: order.targets.clone(),
            distance: 2.,
        },
        Offer {
            issuer: identity(20),
            definition: issuer.clone(),
            local: order.local,
            state: order.state,
            generation: 1,
            targets: order.targets.clone(),
            distance: 2.,
        },
        Offer {
            issuer: Identity {
                world: 2,
                ..identity(15)
            },
            definition: issuer.clone(),
            local: order.local,
            state: order.state,
            generation: 1,
            targets: order.targets.clone(),
            distance: 0.,
        },
    ];
    tick(&mut a, &mut w, 0, 0);
    tick(&mut b, &mut w, 50, 0);
    assert!(
        w.events
            .iter()
            .filter(|e| e.text.starts_with("apply:Protect"))
            .all(|e| e.scope.issuer == 20)
    );
    assert_eq!(w.labels("condition:").len(), 2);
    tick(&mut a, &mut w, 100, 100);
    assert!(!a.has_tag(1));
    tick(&mut a, &mut w, 150, 50);
    assert!(a.has_tag(1));
    let before = w.labels("condition:").len();
    w.offers.clear();
    tick(&mut a, &mut w, 200, 50);
    assert!(a.has_tag(1));
    assert_eq!(w.labels("condition:").len(), before);
    assert!(w.actions().last().unwrap().1.starts_with("apply:Wander"));
    // Listener b still has an independent first-state condition, removed exactly once on issuer invalidation.
    tick(&mut b, &mut w, 200, 50);
    assert_eq!(w.labels("condition:").len(), before + 1);
    assert!(
        w.events
            .iter()
            .any(|e| e.scope.identity.actor == 11 && e.text.ends_with("false"))
    );
}
#[test]
fn xml_runtime_order_failed_movement_does_not_claim_bucket_and_filters_exactly() {
    let (issuer, follower) = ordered_fixture();
    let order = &issuer.states[1].orders[0];
    let mut a = Actor::new(follower, identity(10), 1).unwrap();
    let mut w = Recording::new();
    w.offers = vec![Offer {
        issuer: identity(20),
        definition: issuer.clone(),
        local: order.local,
        state: order.state,
        generation: 1,
        targets: Some(vec![0x600]),
        distance: 1.,
    }];
    tick(&mut a, &mut w, 0, 0);
    assert!(w.actions()[0].1.starts_with("apply:Wander"));
    w.offers[0].targets = Some(vec![0x601]);
    w.blocked.push(K::Protect);
    tick(&mut a, &mut w, 50, 50);
    // The condition is a successful nonmovement operation; its entry does not consume movement bucket.
    assert!(w.actions().last().unwrap().1.starts_with("apply:Wander"));
}
#[test]
fn xml_runtime_terminal_claim_death_hooks_and_cleanup_once() {
    let def = compile(
        r#"<Behavior effect="Invulnerable">ConditionEffect</Behavior><Behavior childId="Piratess" prob="1">MetamorphoseAtDeath</Behavior><Behavior childId="Piratess" min="1" max="2">ReleaseSpawnAtDeath</Behavior><Behavior>VictoryAtDeath</Behavior>"#,
    );
    let mut a = actor(def);
    let mut w = Recording::new();
    tick(&mut a, &mut w, 0, 0);
    a.defeated(&mut w).unwrap();
    a.defeated(&mut w).unwrap();
    tick(&mut a, &mut w, 50, 50);
    assert_eq!(
        w.labels("death:"),
        vec![
            (0, "death:MetamorphoseAtDeath".into()),
            (0, "death:ReleaseSpawnAtDeath".into()),
            (0, "death:VictoryAtDeath".into())
        ]
    );
    assert_eq!(
        w.labels("terminal:"),
        vec![(0, "terminal:Defeated:true:true".into())]
    );
    assert_eq!(w.labels("condition:").len(), 2);
    for (body, reason) in [
        (
            r#"<Behavior time=".1" childId="Piratess">Metamorphose</Behavior>"#,
            "Metamorphose",
        ),
        (r#"<Behavior numShots="0">Explode</Behavior>"#, "Explode"),
    ] {
        let mut a = actor(compile(body));
        let mut w = Recording::new();
        update(
            &mut a,
            &mut w,
            0,
            Frame {
                stunned: true,
                ..Frame::default()
            },
        )
        .unwrap();
        tick(&mut a, &mut w, 100, 100);
        a.defeated(&mut w).unwrap();
        assert_eq!(w.labels("terminal:").len(), 1);
        assert!(w.labels("terminal:")[0].1.contains(reason));
    }
}
#[test]
fn xml_runtime_place_limit_make_minions_initial_offset_and_lifecycle_world_deadline_seam() {
    let def = compile(
        r#"<Behavior childIds="Piratess" maxChildren="2" startWithMinions="false" cooldown=".1">MakeMinions</Behavior><Behavior time="3">Decay</Behavior>"#,
    );
    let mut a = actor(def);
    let mut w = Recording::new();
    for at in [0, 50, 100, 150, 200, 300] {
        tick(&mut a, &mut w, at, if at == 300 { 100 } else { 50 });
    }
    // startWithMinions=false still emits on the first update; Clear() zeroes the cooldown (behavior.cc:1064-1095).
    assert_eq!(
        w.actions().iter().map(|(t, _)| *t).collect::<Vec<_>>(),
        vec![0, 100, 200, 300]
    );
    assert!(!a.retired());
    // World/lifecycle expiry is deliberately not inferred from active clock: host may retire a dormant actor.
    a.cleanup(&mut w).unwrap();
    assert!(a.retired());
    assert!(a.path().is_empty());
}
#[test]
fn xml_runtime_errors_are_contextual_and_poison_against_replay() {
    let mut a = actor(compile("<Behavior>Shoot</Behavior>"));
    let mut w = Recording::new();
    w.fail = true;
    let error = format!("{:#}", a.tick(Frame::default(), &mut w).unwrap_err());
    assert!(
        error.contains("rules.xml")
            && error.contains("object=Pirate")
            && error.contains("actor=10")
            && error.contains("unsupported"),
        "{error}"
    );
    assert!(
        a.tick(Frame::default(), &mut w)
            .unwrap_err()
            .to_string()
            .contains("refusing effect replay")
    );
    assert!(w.actions().is_empty());
    let definition =
        compile(r#"<Behavior cooldownJitter="true" noCooldownJitter="true">Shoot</Behavior>"#);
    assert!(
        format!("{:#}", Actor::new(definition, identity(10), 1).unwrap_err())
            .contains("contradictory")
    );
}
#[test]
fn xml_runtime_jitter_fixed_draw_boundaries_and_zero_no_draw() {
    assert_eq!(jitter_delay(1000, 500., 0.).unwrap(), 500);
    assert_eq!(jitter_delay(1000, 500., 0.5).unwrap(), 1000);
    assert_eq!(jitter_delay(1000, 500., 1. - f64::EPSILON).unwrap(), 1499);
    assert_eq!(jitter_delay(3000, 1000., 0.).unwrap(), 2000);
    assert_eq!(jitter_delay(3000, 1000., 1. - f64::EPSILON).unwrap(), 3999);
    assert_eq!(jitter_delay(10, 20., 0.).unwrap(), 0);
    assert!(jitter_delay(u64::MAX, 1000., 0.9).is_err());
    assert!(jitter_delay(1, 1., f64::NAN).is_err());
    let mut b = compile(r#"<Behavior cooldown="0" cooldownJitter="true">Shoot</Behavior>"#).states
        [0]
    .behaviors[0]
        .clone();
    let mut rng = StepRng::new(0, 7);
    assert_eq!(cooldown(&b, &mut rng).unwrap(), 0);
    assert_eq!(rng.next_u64(), 0);
    b.fields.insert(F::Cooldown, Value::Millis(1000));
    assert_eq!(cooldown(&b, &mut StepRng::new(0, 0)).unwrap(), 500);
    b.fields.insert(F::CooldownJitter, Value::Bool(false));
    let mut rng = StepRng::new(5, 7);
    assert_eq!(cooldown(&b, &mut rng).unwrap(), 1000);
    assert_eq!(rng.next_u64(), 5);
    b.kind = K::TrapTrigger;
    b.fields.insert(F::CooldownJitter, Value::Millis(0));
    let mut rng = StepRng::new(5, 7);
    assert_eq!(cooldown(&b, &mut rng).unwrap(), 1000);
    assert_eq!(rng.next_u64(), 5);
    b.fields.insert(F::CooldownJitter, Value::Millis(100));
    assert_eq!(cooldown(&b, &mut StepRng::new(0, 0)).unwrap(), 900);
}
#[test]
fn xml_runtime_overflow_deadlines_refuse_without_wrap() {
    let mut a = actor(compile(r#"<Behavior cooldown=".1">Shoot</Behavior>"#));
    let mut w = Recording::new();
    tick(&mut a, &mut w, 0, 0);
    a.now = u64::MAX - 10;
    let e = format!(
        "{:#}",
        a.tick(
            Frame {
                delta_ms: 50,
                ..Frame::default()
            },
            &mut w
        )
        .unwrap_err()
    );
    assert!(e.contains("clock overflow"));
    let mut a = actor(compile(r#"<Behavior cooldown=".1">Shoot</Behavior>"#));
    tick(&mut a, &mut w, 0, 0);
    a.now = u64::MAX - 10;
    assert!(
        format!(
            "{:#}",
            a.tick(
                Frame {
                    delta_ms: 0,
                    ..Frame::default()
                },
                &mut w
            )
            .unwrap_err()
        )
        .contains("deadline overflow")
    );
}

#[test]
fn xml_runtime_actor_admission_contradictory_roles_refuse_before_construction() {
    let scripted = PRODUCTION
        .definitions
        .values()
        .find(|d| d.disposition == Disposition::Active)
        .unwrap();
    let native = PRODUCTION
        .definitions
        .values()
        .find(|d| d.disposition == Disposition::Native)
        .unwrap();
    let mut native_as_active = native.clone();
    native_as_active.disposition = Disposition::Active;
    let mut native_as_passive = native.clone();
    native_as_passive.disposition = Disposition::Passive;
    let mut native_without_service = native.clone();
    native_without_service.native_service = None;
    let mut scripted_with_service = scripted.clone();
    scripted_with_service.native_service = Some(NativeService::Scenery);
    for def in [
        native_as_active,
        native_as_passive,
        native_without_service,
        scripted_with_service,
    ] {
        validate(&def).unwrap();
        assert!(def.admission_role().is_err());
        let expected_file = def.source.file.clone();
        let error = format!(
            "{:#}",
            Actor::new(Arc::new(def), identity(10), 12345).unwrap_err()
        );
        assert!(
            error.contains("contradictory admission")
                && error.contains(&expected_file)
                && error.contains("Actor admission"),
            "{error}"
        );
    }
}

#[test]
fn xml_runtime_fixture_admission_blade_dependency_remains_explicit() {
    let raw = crate::content::game::test_inputs::selected([
        (
            "game/123.5.1/game.xml",
            br#"<Game schema="1"><Include file="rules.xml"/><Root object="Pirate"/></Game>"#
                .to_vec(),
        ),
        (
            "game/123.5.1/rules.xml",
            format!(
                "<Objects>{}</Objects>",
                authored("Pirate", 0x600, "<Behavior>Wander</Behavior>")
            )
            .into_bytes(),
        ),
    ]);
    let error = format!("{:#}", game::compile_fixture(&CONTENT, raw).unwrap_err());
    assert!(
        error.contains("objects.xml selected property dependency owner=0x600 Pirate")
            && error.contains("0x6 Blade has no explicit admission"),
        "{error}"
    );
    let game = compile_with(
        &CONTENT,
        authored("Pirate", 0x600, "<Behavior>Wander</Behavior>"),
        "Pirate",
    );
    assert_eq!(
        game.definitions[&6].admission_role().unwrap(),
        AdmissionRole::Native(NativeService::Projectile)
    );
    assert_eq!(game.properties[&6].name, "Blade");
}

#[test]
fn xml_runtime_production_all_definitions_vocabulary_and_current_service_routing() {
    let mut counts = BTreeMap::<String, usize>::new();
    let mut routed = 0;
    let mut active = 0;
    let mut passive = 0;
    let mut native = 0;
    let mut native_classes = BTreeMap::<String, usize>::new();
    let mut non_enemy_scripted = 0;
    for def in PRODUCTION.definitions.values() {
        validate(def).unwrap();
        match def.admission_role().unwrap() {
            AdmissionRole::Native(service) => {
                let Err(error) = Actor::new(Arc::new(def.clone()), identity(10), 12345) else {
                    panic!("Actor::new accepted native descriptor {:#x}", def.kind);
                };
                let message = format!("{error:#}");
                assert!(
                    message.contains(&def.source.file)
                        && message.contains(&def.source.object)
                        && message.contains("Scripted"),
                    "{message}"
                );
                native += 1;
                assert_eq!(def.disposition, Disposition::Native);
                assert!(def.states.iter().all(|s| s.behaviors.is_empty()
                    && s.transitions.is_empty()
                    && s.orders.is_empty()));
                let class = PRODUCTION.properties[&def.kind].class.as_str();
                assert!(
                    match service {
                        NativeService::Inventory => matches!(class, "Equipment" | "Dye"),
                        NativeService::Player => class == "Player",
                        NativeService::Projectile => class == "Projectile",
                        NativeService::Companion | NativeService::Deployment(_) =>
                            class == "Character",
                        NativeService::Portal => matches!(class, "Portal" | "GuildHallPortal"),
                        NativeService::Merchant => matches!(class, "Merchant" | "GuildMerchant"),
                        NativeService::Container =>
                            matches!(class, "Container" | "ClosedVaultChest"),
                        NativeService::Interaction => matches!(
                            class,
                            "MoneyChanger"
                                | "CharacterChanger"
                                | "NameChanger"
                                | "GuildRegister"
                                | "GuildChronicle"
                                | "GuildBoard"
                        ),
                        NativeService::Scenery => matches!(
                            class,
                            "GameObject"
                                | "Wall"
                                | "ConnectedWall"
                                | "SpiderWeb"
                                | "Sign"
                                | "CaveWall"
                        ),
                    },
                    "native descriptor class/service mismatch: {class}/{service:?}"
                );
                *native_classes
                    .entry(format!("{class}/{service:?}"))
                    .or_default() += 1;
                continue;
            }
            AdmissionRole::Scripted => {
                assert!(def.native_service.is_none());
                non_enemy_scripted += usize::from(!PRODUCTION.properties[&def.kind].enemy);
                if def.disposition == Disposition::Passive {
                    passive += 1;
                }
            }
        }
        for b in def.states.iter().flat_map(|s| &s.behaviors) {
            validate_schedule(b)
                .unwrap_or_else(|e| panic!("{}:{}: {e:#}", b.source.file, b.source.line));
            *counts.entry(format!("{:?}", b.kind)).or_default() += 1;
            if service(b.kind).is_some() {
                routed += 1;
            }
        }
        let mut a = actor(Arc::new(def.clone()));
        assert!(!a.started && a.locals.is_empty() && a.path.is_empty() && a.pending.is_none());
        assert_eq!(
            a.rng.clone().next_u64(),
            StdRng::seed_from_u64(12345).next_u64()
        );
        if def.disposition == Disposition::Active {
            active += 1;
            let mut w = Recording::new();
            tick(&mut a, &mut w, 0, 0);
            tick(&mut a, &mut w, 50, 50);
            a.defeated_with_context(TerminalContext::NonPlayer, &mut w)
                .unwrap();
        }
    }
    assert!(non_enemy_scripted > 0);
    assert_eq!(PRODUCTION.definitions.len(), 2021);
    assert_eq!((active, passive, native), (542, 19, 1460));
    assert!(routed > 0);
    let source_xml = std::fs::read_to_string("data/game/123.5.1/actors.xml").unwrap();
    let source_doc = roxmltree::Document::parse(&source_xml).unwrap();
    let source_vocabulary: BTreeSet<_> = source_doc
        .descendants()
        .filter(|n| n.has_tag_name("Behavior"))
        .map(|n| n.text().unwrap().trim().to_owned())
        .collect();
    assert_eq!(source_vocabulary.len(), 50);
    assert_eq!(
        counts.keys().cloned().collect::<BTreeSet<_>>(),
        source_vocabulary
    );
    use sha2::{Digest, Sha256};
    let ir_sha = format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&(
                &PRODUCTION.definitions,
                &PRODUCTION.symbols,
                &PRODUCTION.string_lists
            ))
            .unwrap()
        )
    );
    receipt(
        "xml-runtime-production.json",
        serde_json::json!({"qualification":"534 actual Scripted Active mock-service/clock/death-hook smokes; 19 Scripted Passive and 881 Native catalogue descriptors, NOT 881 extra AI activations/native services/World ownership or historical fidelity", "fingerprint":PRODUCTION.fingerprint,"compiled_behavior_ir_sha256":ir_sha,"compiled_behavior_ir_scope":"compact definitions/symbols/string_lists tuple, not whole production closure","definitions":PRODUCTION.definitions.len(),"active":active,"passive":passive,"native":native,"native_class_services":native_classes,"routed_behaviors":routed,"vocabulary":counts}),
    );
}
#[test]
fn xml_runtime_typed_service_partition_covers_all_54_compiler_operations() {
    let kinds = K::ALL;
    assert_eq!(kinds.len(), 54);
    let control_names: BTreeSet<_> = [
        "ChangeSize",
        "ClearGlobalTag",
        "ClearTag",
        "ConditionEffect",
        "Decay",
        "FollowOrders",
        "OryxTaunt",
        "Metamorphose",
        "MetamorphoseAtDeath",
        "ReleaseSpawnAtDeath",
        "SetGlobalTag",
        "SetTag",
        "VictoryAtDeath",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    assert_eq!(
        kinds
            .iter()
            .filter(|k| service(**k).is_none())
            .map(|k| format!("{k:?}"))
            .collect::<BTreeSet<_>>(),
        control_names
    );
    assert_eq!(kinds.iter().filter(|k| service(**k).is_some()).count(), 41);
    assert_eq!(service(K::Zap), Some(Service::Attack));
    assert_eq!(service(K::PlaySound), Some(Service::Sound));
    assert_eq!(service(K::DeleteObject), Some(Service::Delete));
    assert_matches!(
        compile("<Behavior>Shoot</Behavior>").states[0].behaviors[0].fields[&F::Type],
        Value::Mode(Mode::Targeted)
    );
}

#[test]
fn xml_runtime_load_time_documented_cooldowns_and_explicit_values_win() {
    let rows = [
        ("FollowOrders", "", "followorders", 3000, "3.0"),
        ("Follow", "", "follow", 5000, "5.0"),
        ("Charge", "", "charge", 3000, "3.0"),
        ("StayCloseToSpawn", "", "stayclosetospawn", 3000, "3.0"),
        ("PlaceMap", "map=\"Contract Map\"", "placemap", 5000, "5.0"),
        (
            "ClearConditionEffectAura",
            "effect=\"Stunned\"",
            "clearconditioneffectaura",
            1000,
            "1.0",
        ),
        (
            "ConditionEffectAura",
            "effect=\"Stunned\"",
            "conditioneffectaura",
            1000,
            "1.0",
        ),
        ("Heal", "", "heal", 10000, "10.0"),
        ("HealZap", "", "healzap", 1000, "1.0"),
        ("TrapTrigger", "", "traptrigger", 3000, "3.0"),
        ("Zap", "", "zap", 1000, "1.0"),
        (
            "MakeMinions",
            "childIds=\"Piratess\" maxChildren=\"2\"",
            "makeminions",
            30000,
            "30.0",
        ),
        (
            "MaintainProtectors",
            "childIds=\"Piratess\" numChildren=\"2\"",
            "maintainprotectors",
            30000,
            "30",
        ),
    ];
    let mut body = String::new();
    for (name, attrs, doc, _, literal) in rows {
        let text = std::fs::read_to_string(format!(
            "data/reference/deca-xml-documentation/behaviors-transitions-list/{doc}.md"
        ))
        .unwrap();
        assert!(text.contains(&format!("default **{literal}**")), "{doc}");
        for cooldown in ["", "cooldown=\"0\"", "cooldown=\".25\""] {
            body += &format!("<Behavior {attrs} {cooldown}>{name}</Behavior>");
        }
    }
    let def = compile(&body);
    for (row, (_, _, _, ms, _)) in rows.iter().enumerate() {
        for (column, expected) in [*ms, 0, 250].into_iter().enumerate() {
            assert_eq!(
                time(&def.states[0].behaviors[row * 3 + column], F::Cooldown).unwrap(),
                Some(expected)
            );
        }
    }
    for b in def.states[0]
        .behaviors
        .iter()
        .filter(|b| b.kind == K::FollowOrders)
    {
        assert_eq!(number(b, F::AcquireRange).unwrap(), Some(15.));
    }
    let explicit = compile(r#"<Behavior acquireRange="0" cooldown="0">FollowOrders</Behavior>"#);
    assert_eq!(
        number(&explicit.states[0].behaviors[0], F::AcquireRange).unwrap(),
        Some(0.)
    );
}
#[test]
fn xml_runtime_motion_continues_during_cooldown_without_restart_or_draw() {
    let def = compile(
        r#"<Behavior bucket="movement" cooldown="1" duration=".2">Follow</Behavior><Behavior bucket="movement">Wander</Behavior>"#,
    );
    let local = def.states[0].behaviors[0].local;
    let mut a = actor(def);
    let mut w = Recording::new();
    tick(&mut a, &mut w, 0, 0);
    let mut rng = a.locals[&local].rng.clone();
    w.ongoing = true;
    tick(&mut a, &mut w, 50, 50);
    tick(&mut a, &mut w, 100, 50);
    assert_eq!(w.actions().len(), 1);
    assert_eq!(
        w.labels("advance:"),
        vec![
            (50, "advance:Follow:50".into()),
            (100, "advance:Follow:50".into())
        ]
    );
    assert_eq!(a.locals[&local].next, 1000);
    assert_eq!(a.locals[&local].emissions, 1);
    assert_eq!(a.locals[&local].rng.clone().next_u64(), rng.next_u64());
    w.ongoing = false;
    tick(&mut a, &mut w, 200, 100);
    assert!(w.actions().last().unwrap().1.starts_with("apply:Wander"));
}
#[test]
fn xml_runtime_map_limit_and_bounded_child_failure_dont_spend_cooldown() {
    let def = compile(
        r#"<Behavior map="Contract Map" maxPlace="2" cooldown=".1">PlaceMap</Behavior><Behavior childIds="Piratess" maxChildren="2" cooldown=".1">MakeMinions</Behavior>"#,
    );
    let child = def.states[0].behaviors[1].local;
    let mut a = actor(def);
    let mut w = Recording::new();
    w.blocked.push(K::MakeMinions);
    tick(&mut a, &mut w, 0, 0);
    tick(&mut a, &mut w, 100, 100);
    assert_eq!(a.locals[&child].emissions, 0);
    assert_eq!(a.locals[&child].next, 0);
    w.blocked.clear();
    tick(&mut a, &mut w, 150, 50);
    tick(&mut a, &mut w, 200, 50);
    tick(&mut a, &mut w, 250, 50);
    assert_eq!(
        w.actions()
            .iter()
            .filter(|(_, s)| s.starts_with("apply:PlaceMap"))
            .map(|(t, _)| *t)
            .collect::<Vec<_>>(),
        vec![0, 100]
    );
    assert_eq!(
        w.actions()
            .iter()
            .filter(|(_, s)| s.starts_with("apply:MakeMinions"))
            .map(|(t, _)| *t)
            .collect::<Vec<_>>(),
        vec![150, 250]
    );
}
#[test]
fn xml_runtime_order_reentry_generation_resets_and_cooldown_does_not_delay_cleanup() {
    let (issuer, follower) = ordered_fixture();
    let order = issuer.states[1].orders[0].clone();
    let mut a = actor(follower);
    let mut w = Recording::new();
    w.offers.push(Offer {
        issuer: identity(20),
        definition: issuer,
        local: order.local,
        state: order.state,
        generation: 1,
        targets: order.targets,
        distance: 2.,
    });
    tick(&mut a, &mut w, 0, 0);
    assert_eq!(w.labels("condition:").len(), 1);
    w.offers[0].generation = 2;
    tick(&mut a, &mut w, 50, 50);
    assert_eq!(w.labels("condition:").len(), 2);
    assert!(w.actions().last().unwrap().1.starts_with("apply:Wander"));
    tick(&mut a, &mut w, 3000, 2950);
    assert_eq!(w.labels("condition:").len(), 3);
    assert!(
        w.events
            .iter()
            .filter(|e| e.text.starts_with("condition:") && e.text.ends_with("true"))
            .map(|e| e.scope.generation)
            .eq([1, 2])
    );
}
#[test]
fn xml_runtime_maintain_jitter_and_repeated_local_rng_independence() {
    let def = compile(
        r#"<DefineEmbed name="Pulse"><Behavior type="auto" cooldown="1" cooldownJitter="true">Shoot</Behavior></DefineEmbed><Embed name="Pulse"/><Embed name="Pulse"/>"#,
    );
    let ids: Vec<_> = def.states[0].behaviors.iter().map(|b| b.local).collect();
    let mut a = actor(def);
    let mut w = Recording::new();
    tick(&mut a, &mut w, 0, 0);
    for id in ids {
        let mut independent = StdRng::seed_from_u64(local_seed(12345, id));
        let u = independent.r#gen::<f64>();
        assert_eq!(a.locals[&id].next, (500. + 1000. * u).floor() as u64);
        assert_eq!(a.locals[&id].rng.clone().next_u64(), independent.next_u64());
    }
    let mut b=compile(r#"<Behavior childIds="Piratess" numChildren="1" cooldown="1">MaintainProtectors</Behavior>"#).states[0].behaviors[0].clone();
    // behavior.cc arms the respawn with the exact cooldown; the constructor's jittered value is overwritten.
    let mut rng = StepRng::new(0, 7);
    assert_eq!(cooldown(&b, &mut rng).unwrap(), 1000);
    assert_eq!(rng.next_u64(), 0);
    b.fields.insert(F::Cooldown, Value::Millis(0));
    let mut rng = StepRng::new(0, 7);
    assert_eq!(cooldown(&b, &mut rng).unwrap(), 0);
    assert_eq!(rng.next_u64(), 0);
}
#[test]
fn xml_runtime_unresolved_tildeath_refuses_before_any_actor_effect() {
    let def = compile(r#"<Behavior time=".1" tilDeath="true">Decay</Behavior>"#);
    let err = format!("{:#}", Actor::new(def, identity(10), 1).unwrap_err());
    assert!(err.contains("tilDeath semantics unresolved") && err.contains("rules.xml"));
}
#[test]
fn xml_runtime_every_service_operation_reaches_real_compiler_typed_fallible_seam() {
    let synthetic = compile(
        r#"<Behavior objectId="Pirate" radius="1">DeleteObject</Behavior><Behavior soundId="1">PlaySound</Behavior><Behavior>TrapTrigger</Behavior><Behavior>Zap</Behavior><Behavior>Shadow</Behavior>"#,
    );
    let mut exemplars = BTreeMap::new();
    for b in PRODUCTION
        .definitions
        .values()
        .flat_map(|d| d.states.iter().flat_map(|s| &s.behaviors))
        .chain(&synthetic.states[0].behaviors)
    {
        // This is a routing smoke, so choose guaranteed rows; chance failures have separate RNG tests.
        if service(b.kind).is_some() && number(b, F::Prob).unwrap().is_none_or(|p| p == 1.) {
            exemplars
                .entry(format!("{:?}", b.kind))
                .or_insert_with(|| b.clone());
        }
    }
    assert_eq!(exemplars.len(), 41);
    for (name, b) in exemplars {
        let mut def = (*synthetic).clone();
        def.states.truncate(1);
        def.states[0].children.clear();
        def.states[0].orders.clear();
        def.states[0].transitions.clear();
        def.states[0].behaviors = vec![b.clone()];
        let at = [F::Offset, F::InitialDelay, F::Cooldown]
            .into_iter()
            .filter_map(|f| time(&b, f).unwrap())
            .max()
            .unwrap_or(0);
        let mut a = actor(Arc::new(def));
        let mut w = Recording::new();
        tick(&mut a, &mut w, 0, 0);
        if w.actions().is_empty() {
            tick(&mut a, &mut w, at, at);
        }
        assert_eq!(w.actions().len(), 1, "{name}");
        assert!(w.actions()[0].1.starts_with(&format!("apply:{name}:")));
    }
}

#[test]
fn xml_runtime_atomic_terminal_rejection_retry_and_competing_triggers_do_not_resample() {
    let def = compile(
        r#"<Behavior childId="Piratess" prob=".5">MetamorphoseAtDeath</Behavior><Behavior childId="Piratess" min="1" max="2">ReleaseSpawnAtDeath</Behavior><Behavior>VictoryAtDeath</Behavior><Behavior time=".1" dropLoot="false" giveXP="true">Decay</Behavior>"#,
    );
    let death = def.states[0].behaviors[0].local;
    let mut a = actor(def);
    let mut w = Recording::new();
    tick(&mut a, &mut w, 0, 0);
    w.fail_terminal = true;
    w.time = 100;
    let deadline = w.deadlines[0].0;
    assert!(a.expire_lifecycle(deadline, &mut w).is_err());
    assert!(!a.retired());
    assert!(w.labels("death:").is_empty());
    assert!(w.labels("terminal:").is_empty());
    assert_eq!(a.pending.as_ref().unwrap().reason, TerminalReason::Decay);
    let mut rng = a.locals[&death].rng.clone();
    assert!(a.defeated(&mut w).is_err());
    assert!(a.retry_terminal(&mut w).is_err());
    assert_eq!(a.locals[&death].rng.clone().next_u64(), rng.next_u64());
    w.fail_terminal = false;
    a.retry_terminal(&mut w).unwrap();
    a.retry_terminal(&mut w).unwrap();
    a.defeated(&mut w).unwrap();
    tick(&mut a, &mut w, 150, 50);
    assert!(a.retired());
    assert_eq!(w.terminal_attempts.len(), 3);
    assert!(w.terminal_attempts.windows(2).all(|p| p[0] == p[1]));
    assert_eq!(
        w.labels("terminal:"),
        vec![(100, "terminal:Decay:false:true".into())]
    );
    assert_eq!(
        w.labels("death:")
            .iter()
            .filter(|(_, s)| s.contains("ReleaseSpawnAtDeath"))
            .count(),
        1
    );
    assert_eq!(
        w.labels("death:")
            .iter()
            .filter(|(_, s)| s.contains("VictoryAtDeath"))
            .count(),
        1
    );
    receipt(
        "xml-runtime-terminal-retry.json",
        serde_json::json!({"qualification":"Atomic mock coordinator contract only; no durable account/reward acceptance", "attempts":w.terminal_attempts,"terminal":w.labels("terminal:"),"hooks":w.labels("death:")}),
    );
}
#[test]
fn xml_runtime_decay_explode_births_transform_transfer_and_non_death_cleanup() {
    for (operation, reason, hooks, loot, xp) in [
        (
            r#"<Behavior time="0">Decay</Behavior>"#,
            TerminalReason::Decay,
            1,
            false,
            false,
        ),
        (
            r#"<Behavior numShots="0" dropLoot="true">Explode</Behavior>"#,
            TerminalReason::Explode,
            1,
            true,
            false,
        ),
        (
            r#"<Behavior time="0" childId="Piratess" giveXP="true">Metamorphose</Behavior>"#,
            TerminalReason::Metamorphose,
            0,
            false,
            true,
        ),
    ] {
        let def = compile(
            &(String::from(r#"<Behavior childId="Piratess">ReleaseSpawnAtDeath</Behavior>"#)
                + operation),
        );
        let mut a = actor(def);
        let mut w = Recording::new();
        tick(&mut a, &mut w, 0, 0);
        a.defeated(&mut w).unwrap();
        assert_eq!(w.labels("death:").len(), hooks);
        assert_eq!(
            w.labels("terminal:"),
            vec![(0, format!("terminal:{reason:?}:{loot}:{xp}"))]
        );
        assert!(w.events.iter().all(|e| e.scope.identity == identity(10)));
        if reason == TerminalReason::Explode {
            assert!(w.actions().is_empty());
        }
    }
    let mut a = actor(compile(
        r#"<Behavior childId="Piratess">ReleaseSpawnAtDeath</Behavior>"#,
    ));
    let mut w = Recording::new();
    a.cleanup(&mut w).unwrap();
    assert!(w.labels("death:").is_empty());
    assert!(w.labels("terminal:").is_empty());
}
#[test]
fn xml_runtime_native_nondeath_removal_stops_actions_without_death_hooks() {
    let def = compile(
        r#"<Behavior>RemoveOnEmpty</Behavior><Behavior>VictoryAtDeath</Behavior><Behavior>Shoot</Behavior>"#,
    );
    let mut a = actor(def);
    let mut w = Recording::new();
    w.remove = true;
    tick(&mut a, &mut w, 0, 0);
    a.defeated(&mut w).unwrap();
    assert_eq!(w.actions().len(), 1);
    assert!(w.actions()[0].1.starts_with("apply:RemoveOnEmpty:"));
    assert!(a.retired());
    assert!(a.path().is_empty());
    assert!(w.labels("death:").is_empty());
    assert!(w.labels("terminal:").is_empty());
}

#[test]
fn xml_runtime_size_overflow_refuses_without_infinite_clamp() {
    let mut a = actor(compile(r#"<Behavior rate="1e308">ChangeSize</Behavior>"#));
    let mut w = Recording::new();
    tick(&mut a, &mut w, 0, 0);
    assert!(
        format!(
            "{:#}",
            a.tick(
                Frame {
                    delta_ms: 1000,
                    ..Frame::default()
                },
                &mut w
            )
            .unwrap_err()
        )
        .contains("size change overflow")
    );
}

#[test]
fn xml_runtime_lifecycle_world_deadline_ignores_stasis_dormancy_without_advancing_ai() {
    for (operation, reason) in [
        (
            r#"<Behavior time="1">Decay</Behavior>"#,
            TerminalReason::Decay,
        ),
        (
            r#"<Behavior time="1" childId="Piratess">Metamorphose</Behavior>"#,
            TerminalReason::Metamorphose,
        ),
    ] {
        for paused in [
            Frame {
                stasis: true,
                delta_ms: 1000,
                ..Frame::default()
            },
            Frame {
                dormant: true,
                delta_ms: 1000,
                ..Frame::default()
            },
        ] {
            let def =
                compile(&(operation.to_owned() + r#"<Behavior type="auto">Shoot</Behavior>"#));
            let mut a = actor(def);
            let mut w = Recording::new();
            update(
                &mut a,
                &mut w,
                0,
                Frame {
                    delta_ms: 0,
                    ..paused
                },
            )
            .unwrap();
            assert_eq!(w.deadlines.len(), 1);
            assert!(w.actions().is_empty());
            update(&mut a, &mut w, 1000, paused).unwrap();
            assert_eq!(a.active_ms(), 0);
            assert!(a.retired());
            assert!(w.actions().is_empty());
            assert_eq!(
                w.labels("terminal:"),
                vec![(1000, format!("terminal:{reason:?}:false:false"))]
            );
        }
    }
}

#[test]
fn xml_runtime_decay_cancel_reentry_stale_generation_and_world_isolation() {
    let def = compile(
        r#"<State id="A"><Behavior time=".2">Decay</Behavior><Transition afterTime=".1">B</Transition></State><State id="B"><Transition afterTime=".1">A</Transition></State>"#,
    );
    let mut a = actor(def);
    let mut w = Recording::new();
    tick(&mut a, &mut w, 0, 0);
    let old = w.deadlines[0].0;
    tick(&mut a, &mut w, 100, 100);
    assert!(w.deadlines.is_empty());
    tick(&mut a, &mut w, 200, 100);
    assert_ne!(w.deadlines[0].0.generation, old.generation);
    assert!(!a.expire_lifecycle(old, &mut w).unwrap());
    assert!(!a.retired());
    let wrong = Deadline {
        owner: Token {
            scope: Scope {
                identity: Identity {
                    world: 2,
                    ..identity(10)
                },
                ..a.scope
            },
            local: old.owner.local,
        },
        ..old
    };
    assert!(
        a.expire_lifecycle(wrong, &mut w)
            .unwrap_err()
            .to_string()
            .contains("another actor/world")
    );
}
#[test]
fn xml_runtime_order_explode_and_decay_merge_listener_hooks_atomically() {
    for ending in [
        r#"<Behavior numShots="0">Explode</Behavior>"#,
        r#"<Behavior time=".1">Decay</Behavior>"#,
    ] {
        let rules = authored(
            "Pirate",
            0x600,
            &format!(
                r#"<Order targetIds="Piratess"><Behavior childId="Pirate">ReleaseSpawnAtDeath</Behavior>{ending}</Order>"#
            ),
        ) + &authored(
            "Piratess",
            0x601,
            r#"<Behavior listenIds="Pirate">FollowOrders</Behavior><Behavior>VictoryAtDeath</Behavior>"#,
        );
        let mut game = compile_with(&CONTENT, rules, "Pirate");
        let issuer = Arc::new(game.definitions.remove(&0x600).unwrap());
        let order = &issuer.states[0].orders[0];
        let mut a = actor(Arc::new(game.definitions.remove(&0x601).unwrap()));
        let mut w = Recording::new();
        w.offers.push(offer(&issuer, order));
        tick(&mut a, &mut w, 0, 0);
        let delayed = ending.contains("Decay");
        if delayed {
            assert_eq!(w.deadlines.len(), 1);
            update(
                &mut a,
                &mut w,
                100,
                Frame {
                    stasis: true,
                    delta_ms: 100,
                    ..Frame::default()
                },
            )
            .unwrap();
        }
        assert!(a.retired());
        assert_eq!(w.terminal_attempts.len(), 1);
        if delayed {
            assert_eq!(w.labels("death:").len(), 2);
        } else {
            assert_eq!(
                w.labels("death:")
                    .iter()
                    .map(|(_, s)| s.as_str())
                    .collect::<Vec<_>>(),
                ["death:VictoryAtDeath", "death:ReleaseSpawnAtDeath"]
            );
            assert!(
                w.events
                    .iter()
                    .filter(|e| e.text.starts_with("terminal:"))
                    .all(|e| e.scope.issuer == 10)
            );
        }
    }
}

#[test]
fn xml_runtime_transform_state_exit_cancellation_reentry_generation_and_order_invalidation() {
    let def = compile(
        r#"<State id="A"><Behavior time=".2" childId="Piratess">Metamorphose</Behavior><Transition afterTime=".1">B</Transition></State><State id="B"><Transition afterTime=".1">A</Transition></State>"#,
    );
    let mut a = actor(def);
    let mut w = Recording::new();
    tick(&mut a, &mut w, 0, 0);
    let old = w.deadlines[0].0;
    tick(&mut a, &mut w, 100, 100);
    assert!(w.deadlines.is_empty());
    tick(&mut a, &mut w, 200, 100);
    assert_ne!(w.deadlines[0].0.generation, old.generation);
    assert!(!a.expire_lifecycle(old, &mut w).unwrap());
    update(
        &mut a,
        &mut w,
        400,
        Frame {
            stasis: true,
            delta_ms: 200,
            ..Frame::default()
        },
    )
    .unwrap();
    assert_eq!(
        w.labels("terminal:"),
        vec![(400, "terminal:Metamorphose:false:false".into())]
    );
    let rules = authored(
        "Pirate",
        0x600,
        r#"<Order targetIds="Piratess"><Behavior time=".1" childId="Pirate">Metamorphose</Behavior></Order>"#,
    ) + &authored(
        "Piratess",
        0x601,
        r#"<Behavior listenIds="Pirate">FollowOrders</Behavior>"#,
    );
    let mut game = compile_with(&CONTENT, rules, "Pirate");
    let issuer = Arc::new(game.definitions.remove(&0x600).unwrap());
    let order = &issuer.states[0].orders[0];
    let mut a = actor(Arc::new(game.definitions.remove(&0x601).unwrap()));
    let mut w = Recording::new();
    w.offers.push(offer(&issuer, order));
    tick(&mut a, &mut w, 0, 0);
    let old = w.deadlines[0].0;
    w.offers.clear();
    // No listener AI tick occurs: deadline discovery itself must reject the retired issuer/order.
    w.time = 100;
    assert!(!a.expire_lifecycle(old, &mut w).unwrap());
    assert!(w.deadlines.is_empty());
    assert!(!a.retired());
}
#[test]
fn xml_runtime_transform_failed_atomic_plan_competes_with_decay_death_without_hooks_or_resampling()
{
    let def = compile(
        r#"<Behavior childId="Piratess" prob=".5">MetamorphoseAtDeath</Behavior><Behavior time=".1" childId="Piratess" dropLoot="true">Metamorphose</Behavior><Behavior time=".1" giveXP="true">Decay</Behavior>"#,
    );
    let hook = def.states[0].behaviors[0].local;
    let mut a = actor(def);
    let mut w = Recording::new();
    tick(&mut a, &mut w, 0, 0);
    let transform = w.deadlines[0].0;
    let decay = w.deadlines[1].0;
    let mut rng = a.locals[&hook].rng.clone();
    w.fail_terminal = true;
    w.time = 100;
    assert!(a.expire_lifecycle(transform, &mut w).is_err());
    assert!(a.expire_lifecycle(decay, &mut w).is_err());
    assert!(a.defeated(&mut w).is_err());
    assert!(a.retry_terminal(&mut w).is_err());
    assert_eq!(a.locals[&hook].rng.clone().next_u64(), rng.next_u64());
    assert!(a.pending.as_ref().unwrap().hooks.is_empty());
    assert_eq!(
        a.pending.as_ref().unwrap().reason,
        TerminalReason::Metamorphose
    );
    w.fail_terminal = false;
    a.retry_terminal(&mut w).unwrap();
    assert!(!a.expire_lifecycle(decay, &mut w).unwrap());
    a.defeated(&mut w).unwrap();
    assert!(w.terminal_attempts.windows(2).all(|p| p[0] == p[1]));
    assert!(w.labels("death:").is_empty());
    assert!(w.deadlines.is_empty());
    assert_eq!(
        w.labels("terminal:"),
        vec![(100, "terminal:Metamorphose:true:false".into())]
    );
}

#[test]
fn xml_runtime_blocked_attempt_rewinds_only_eligible_local_and_allows_bucket_alternative() {
    let def = compile(
        r#"<Behavior bucket="attempt" cooldown="1" cooldownJitter="true">Shoot</Behavior><Behavior bucket="attempt" cooldown=".05">Charge</Behavior><State id="A"><Transition afterTime=".1">B</Transition></State><State id="B"/>"#,
    );
    let blocked = def.states[0].behaviors[0].local;
    let alternative = def.states[0].behaviors[1].local;
    let destination = def.states.iter().find(|s| s.name == "B").unwrap().id;
    let mut expected = StdRng::seed_from_u64(local_seed(12345, blocked));
    let proposal = expected.next_u64();
    let initial = StdRng::seed_from_u64(local_seed(12345, blocked)).next_u64();
    let mut other = StdRng::seed_from_u64(local_seed(12345, alternative));
    let mut a = actor(def);
    let mut w = Recording::new();
    w.reject.push(K::Shoot);
    w.draw_native = true;
    for at in [0, 50, 100] {
        tick(&mut a, &mut w, at, if at == 0 { 0 } else { 50 });
        let local = &a.locals[&blocked];
        assert_eq!(
            (local.emissions, local.opportunities, local.next),
            (0, 0, 0)
        );
        assert_eq!(local.rng.clone().next_u64(), initial);
        other.next_u64();
    }
    assert_eq!(a.now, 100);
    assert_eq!(a.path.last(), Some(&destination));
    assert!(!a.failed && !a.retired && a.pending.is_none());
    assert_eq!(a.locals[&alternative].emissions, 3);
    assert_eq!(a.locals[&alternative].next, 150);
    assert_eq!(
        a.locals[&alternative].rng.clone().next_u64(),
        other.next_u64()
    );
    assert_eq!(w.actions().len(), 3);
    assert!(
        w.actions()
            .iter()
            .all(|(_, s)| s.starts_with("apply:Charge:"))
    );
    let attempts: Vec<_> = w
        .native_attempts
        .iter()
        .filter(|(k, _, _)| *k == K::Shoot)
        .collect();
    assert_eq!(attempts.len(), 3);
    assert!(
        attempts
            .iter()
            .all(|(_, step, draw)| step.emissions == 0 && *draw == Some(proposal))
    );
    assert!(
        !w.events
            .iter()
            .any(|e| e.text == format!("forget:{blocked}"))
    );
    assert!(w.terminal_plans.is_empty() && w.terminal_claims.is_empty());
    w.reject.clear();
    tick(&mut a, &mut w, 150, 50);
    let jitter = (500. + 1000. * expected.r#gen::<f64>()).floor() as u64;
    assert_eq!(
        (a.locals[&blocked].emissions, a.locals[&blocked].next),
        (1, 150 + jitter)
    );
    assert_eq!(
        a.locals[&blocked].rng.clone().next_u64(),
        expected.next_u64()
    );
    assert_eq!(w.native_attempts.last().unwrap().2, Some(proposal));
    assert_eq!(a.locals[&alternative].emissions, 3);
    assert_eq!(w.actions().len(), 4);
}

#[test]
fn xml_runtime_blocked_say_retries_identical_chance_text_native_payload_then_spends_once() {
    let def = compile(r#"<Behavior prob=".999999" text="a;b;c" cooldown=".1">Say</Behavior>"#);
    let id = def.states[0].behaviors[0].local;
    let mut expected = StdRng::seed_from_u64(local_seed(12345, id));
    let initial = expected.clone().next_u64();
    assert!(expected.r#gen::<f64>() < 0.999999);
    let choice = expected.gen_range(0..3);
    let proposal = expected.next_u64();
    let mut a = actor(def);
    let mut w = Recording::new();
    w.reject.push(K::Say);
    w.draw_native = true;
    for at in [0, 50] {
        tick(&mut a, &mut w, at, if at == 0 { 0 } else { 50 });
        assert_eq!(a.locals[&id].rng.clone().next_u64(), initial);
        assert_eq!(
            (
                a.locals[&id].emissions,
                a.locals[&id].opportunities,
                a.locals[&id].next
            ),
            (0, 0, 0)
        );
        assert!(w.actions().is_empty() && w.say_choices.is_empty());
    }
    w.reject.clear();
    tick(&mut a, &mut w, 100, 50);
    assert_eq!(w.native_attempts.len(), 3);
    assert!(
        w.native_attempts
            .iter()
            .all(|(_, step, draw)| step.text_choice == Some(choice)
                && step.emissions == 0
                && *draw == Some(proposal))
    );
    assert_eq!(w.say_choices, vec![(100, choice)]);
    assert_eq!(
        (
            a.locals[&id].emissions,
            a.locals[&id].opportunities,
            a.locals[&id].next
        ),
        (1, 1, 200)
    );
    assert_eq!(a.locals[&id].rng.clone().next_u64(), expected.next_u64());
    tick(&mut a, &mut w, 150, 50);
    assert_eq!(w.native_attempts.len(), 3);
    tick(&mut a, &mut w, 200, 50);
    assert_eq!(w.actions().len(), 2);
    assert_eq!(a.locals[&id].next, 300);
}

#[test]
fn xml_runtime_blocked_motion_unblocks_with_same_draw_deadline_and_preserves_advance() {
    let def = compile(r#"<Behavior cooldown="1">Follow</Behavior>"#);
    let id = def.states[0].behaviors[0].local;
    let mut expected = StdRng::seed_from_u64(local_seed(12345, id));
    let proposal = expected.next_u64();
    let mut a = actor(def);
    let mut w = Recording::new();
    w.reject.push(K::Follow);
    w.draw_native = true;
    for at in [0, 50, 100] {
        tick(&mut a, &mut w, at, if at == 0 { 0 } else { 50 });
        assert_eq!((a.locals[&id].emissions, a.locals[&id].next), (0, 0));
    }
    w.reject.clear();
    tick(&mut a, &mut w, 150, 50);
    assert_eq!(w.actions().len(), 1);
    assert_eq!(w.native_attempts.len(), 4);
    assert!(
        w.native_attempts
            .iter()
            .all(|(_, _, draw)| *draw == Some(proposal))
    );
    assert_eq!((a.locals[&id].emissions, a.locals[&id].next), (1, 1150));
    assert_eq!(
        a.locals[&id].rng.clone().next_u64(),
        expected.clone().next_u64()
    );
    w.ongoing = true;
    w.reject.push(K::Follow);
    tick(&mut a, &mut w, 200, 50);
    assert_eq!(w.native_attempts.len(), 4);
    assert_eq!(w.labels("advance:").len(), 1);
    assert_eq!((a.locals[&id].emissions, a.locals[&id].next), (1, 1150));
    assert_eq!(a.locals[&id].rng.clone().next_u64(), expected.next_u64());
}

#[test]
fn xml_runtime_blocked_native_removal_and_explode_do_not_cleanup_or_claim_terminal() {
    for (kind, name) in [(K::RemoveOnEmpty, "RemoveOnEmpty"), (K::Explode, "Explode")] {
        let fields = if kind == K::Explode {
            r#" projectileId="0""#
        } else {
            ""
        };
        let def = compile(&format!(
            "<Behavior{fields}>{name}</Behavior><Behavior>VictoryAtDeath</Behavior>"
        ));
        let id = def.states[0].behaviors[0].local;
        let mut a = actor(def);
        let mut w = Recording::new();
        w.reject.push(kind);
        w.draw_native = true;
        for at in [0, 50] {
            tick(&mut a, &mut w, at, if at == 0 { 0 } else { 50 });
            assert!(!a.failed && !a.retired && a.pending.is_none());
            assert_eq!((a.locals[&id].emissions, a.locals[&id].next), (0, 0));
            assert!(w.actions().is_empty());
            assert!(w.labels("forget:").is_empty() && w.labels("exit:").is_empty());
            assert!(w.terminal_plans.is_empty() && w.terminal_claims.is_empty());
        }
        w.reject.clear();
        w.remove = kind == K::RemoveOnEmpty;
        tick(&mut a, &mut w, 100, 50);
        assert!(a.retired && a.path.is_empty());
        assert_eq!(w.actions().len(), 1);
        assert_eq!(w.terminal_claims.len(), usize::from(kind == K::Explode));
        assert_eq!(w.labels("death:").len(), usize::from(kind == K::Explode));
    }
}

#[test]
fn xml_runtime_blocked_contract_does_not_rewind_genuine_native_errors() {
    let def = compile(r#"<Behavior text="a;b;c">Say</Behavior>"#);
    let id = def.states[0].behaviors[0].local;
    let mut expected = StdRng::seed_from_u64(local_seed(12345, id));
    let _: usize = expected.gen_range(0..3);
    expected.next_u64();
    let mut a = actor(def);
    let mut w = Recording::new();
    w.fail = true;
    w.draw_native = true;
    let error = format!("{:#}", a.tick(Frame::default(), &mut w).unwrap_err());
    assert!(error.contains("native service unsupported: Say") && error.contains("rules.xml"));
    assert!(a.failed && !a.retired && a.pending.is_none());
    assert_eq!(a.locals[&id].rng.clone().next_u64(), expected.next_u64());
    assert_eq!(
        (
            a.locals[&id].emissions,
            a.locals[&id].opportunities,
            a.locals[&id].next
        ),
        (0, 0, 0)
    );
    w.fail = false;
    assert!(
        format!("{:#}", a.tick(Frame::default(), &mut w).unwrap_err())
            .contains("previously failed")
    );
    assert_eq!(w.native_attempts.len(), 1);
    assert!(w.actions().is_empty() && w.terminal_claims.is_empty());
}

#[test]
fn xml_runtime_blocked_contract_preserves_say_chance_miss_rng_opportunity_and_deadline() {
    let def = compile(r#"<Behavior prob=".05" text="a;b;c" cooldown=".1">Say</Behavior>"#);
    let id = def.states[0].behaviors[0].local;
    let mut expected = StdRng::seed_from_u64(local_seed(12345, id));
    assert!(expected.r#gen::<f64>() >= 0.05);
    let mut a = actor(def);
    let mut w = Recording::new();
    w.reject.push(K::Say);
    w.draw_native = true;
    tick(&mut a, &mut w, 0, 0);
    assert_eq!(
        (
            a.locals[&id].emissions,
            a.locals[&id].opportunities,
            a.locals[&id].next
        ),
        (0, 1, 100)
    );
    assert_eq!(
        a.locals[&id].rng.clone().next_u64(),
        expected.clone().next_u64()
    );
    tick(&mut a, &mut w, 50, 50);
    assert_eq!(a.locals[&id].rng.clone().next_u64(), expected.next_u64());
    assert!(w.native_attempts.is_empty() && w.say_choices.is_empty() && w.actions().is_empty());
    assert!(!a.failed);
}

#[test]
fn xml_runtime_zero_shot_explode_readiness_precedes_rng_plan_and_bucket_claim() {
    let def = compile(
        r#"<Behavior bucket="activation" range="2" numShots="0">Explode</Behavior><Behavior bucket="activation">Wander</Behavior><Behavior childId="Piratess" prob=".5">MetamorphoseAtDeath</Behavior><Behavior childId="Piratess" min="2" max="7">ReleaseSpawnAtDeath</Behavior>"#,
    );
    let explode = def.states[0].behaviors[0].local;
    let alternative = def.states[0].behaviors[1].local;
    let hook = def.states[0].behaviors[2].local;
    let mut a = actor(def);
    let mut w = Recording::new();
    w.blocked.push(K::Explode);
    let mut expected = StdRng::seed_from_u64(local_seed(12345, hook));
    for at in [0, 50, 100] {
        tick(&mut a, &mut w, at, if at == 0 { 0 } else { 50 });
        assert!(!a.failed && !a.retired && a.pending.is_none());
        assert!(w.terminal_plans.is_empty() && w.terminal_claims.is_empty());
        assert!(w.labels("death:").is_empty() && w.labels("forget:").is_empty());
        for id in [explode, hook] {
            let local = &a.locals[&id];
            assert_eq!(
                (local.emissions, local.opportunities, local.next),
                (0, 0, 0)
            );
            assert_eq!(
                local.rng.clone().next_u64(),
                StdRng::seed_from_u64(local_seed(12345, id)).next_u64()
            );
        }
    }
    assert_eq!(a.now, 100);
    assert_eq!(a.locals[&alternative].emissions, 3);
    assert_eq!(w.actions().len(), 3);
    assert!(
        w.actions()
            .iter()
            .all(|(_, s)| s.starts_with("apply:Wander:"))
    );
    let calls = w.ready_calls.borrow();
    let attempts: Vec<_> = calls.iter().filter(|(_, k, _)| *k == K::Explode).collect();
    assert_eq!(attempts.len(), 3);
    assert!(
        attempts
            .iter()
            .all(|(s, _, step)| *s == Service::Attack && step.emissions == 0)
    );
    drop(calls);
    w.blocked.clear();
    w.fail_terminal = true;
    assert!(
        update(
            &mut a,
            &mut w,
            150,
            Frame {
                delta_ms: 50,
                ..Frame::default()
            }
        )
        .is_err()
    );
    let plan = a.pending.clone().unwrap();
    assert_eq!(plan.reason, TerminalReason::Explode);
    assert_eq!(plan.context, Some(TerminalContext::NonPlayer));
    let passed = expected.r#gen::<f64>() < 0.5;
    assert_eq!(
        plan.hooks
            .iter()
            .filter(|h| h.behavior.kind == K::MetamorphoseAtDeath)
            .count(),
        usize::from(passed)
    );
    assert_eq!(
        a.locals[&hook].rng.clone().next_u64(),
        expected.clone().next_u64()
    );
    assert_eq!(a.locals[&alternative].emissions, 3);
    assert!(w.terminal_claims.is_empty());
    let attempts = w.ready_calls.borrow().len();
    w.blocked.push(K::Explode);
    w.players.clear();
    assert!(
        a.defeated_with_context(TerminalContext::NonPlayer, &mut w)
            .is_err()
    );
    assert!(a.retry_terminal(&mut w).is_err());
    assert_eq!(
        format!("{:?}", a.pending.as_ref().unwrap()),
        format!("{plan:?}")
    );
    assert_eq!(a.locals[&hook].rng.clone().next_u64(), expected.next_u64());
    w.fail_terminal = false;
    a.retry_terminal(&mut w).unwrap();
    assert_eq!(w.ready_calls.borrow().len(), attempts);
    assert!(
        w.terminal_plans
            .iter()
            .all(|p| format!("{p:?}") == format!("{plan:?}"))
    );
    assert_eq!(w.terminal_claims.len(), 1);
    assert!(a.retired && a.pending.is_none());
    a.defeated_with_context(TerminalContext::NonPlayer, &mut w)
        .unwrap();
    assert_eq!(w.terminal_plans.len(), 3);
}

#[test]
fn xml_runtime_zero_shot_explode_delay_stun_and_frozen_clocks_preserve_admission() {
    let def = compile(
        r#"<Behavior range="0" numShots="0" initialDelay=".25">Explode</Behavior><Behavior time="10">Decay</Behavior><Behavior childId="Piratess" min="2" max="7">ReleaseSpawnAtDeath</Behavior>"#,
    );
    let mut a = actor(def);
    let mut w = Recording::new();
    assert!(w.players.is_empty());
    for frame in [
        Frame {
            stasis: true,
            delta_ms: 1000,
            ..Frame::default()
        },
        Frame {
            dormant: true,
            delta_ms: 1000,
            ..Frame::default()
        },
    ] {
        a.tick(frame, &mut w).unwrap();
        assert!(a.started && a.now == 0);
        assert_eq!(w.deadlines.len(), 1);
        assert!(w.ready_calls.borrow().is_empty() && w.terminal_plans.is_empty());
    }
    a.tick(
        Frame {
            stunned: true,
            delta_ms: 249,
            ..Frame::default()
        },
        &mut w,
    )
    .unwrap();
    assert_eq!(a.now, 249);
    assert!(w.ready_calls.borrow().is_empty());
    let deadline = w.deadlines[0].0;
    a.tick(
        Frame {
            stunned: true,
            delta_ms: 1,
            ..Frame::default()
        },
        &mut w,
    )
    .unwrap();
    assert_eq!(a.now, 250);
    assert!(a.retired);
    let calls = w.ready_calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        (calls[0].0, calls[0].1, calls[0].2.age_ms),
        (Service::Attack, K::Explode, 250)
    );
    drop(calls);
    assert!(w.native_attempts.is_empty());
    assert_eq!(w.terminal_plans.len(), 1);
    assert_eq!(
        w.terminal_plans[0].context,
        Some(TerminalContext::NonPlayer)
    );
    assert!(!a.expire_lifecycle(deadline, &mut w).unwrap());
    assert_eq!(w.terminal_plans.len(), 1);

    let mut a = actor(compile(
        r#"<Behavior range="0" numShots="1" projectileId="0" initialDelay=".25">Explode</Behavior>"#,
    ));
    let mut w = Recording::new();
    a.tick(Frame::default(), &mut w).unwrap();
    a.tick(
        Frame {
            stunned: true,
            delta_ms: 250,
            ..Frame::default()
        },
        &mut w,
    )
    .unwrap();
    // Recovered ExplodeBehavior::Update never checks STUNNED (behavior.cc), so a stunned host still explodes on time.
    assert!(a.retired && a.now == 250);
    assert_eq!(w.native_attempts.len(), 1);
    assert_eq!(w.native_attempts[0].0, K::Explode);
}

#[test]
fn xml_runtime_zero_shot_explode_readiness_error_is_contextual_and_precedes_sampling() {
    let def = compile(
        r#"<Behavior range="2" numShots="0">Explode</Behavior><Behavior childId="Piratess" prob=".5">MetamorphoseAtDeath</Behavior>"#,
    );
    let hook = def.states[0].behaviors[1].local;
    let mut a = actor(def);
    let mut w = Recording::new();
    w.fail_ready = true;
    let error = format!("{:#}", a.tick(Frame::default(), &mut w).unwrap_err());
    assert!(error.contains("native readiness rejected: Explode") && error.contains("rules.xml"));
    assert!(a.failed && !a.retired && a.pending.is_none());
    assert_eq!(
        a.locals[&hook].rng.clone().next_u64(),
        StdRng::seed_from_u64(local_seed(12345, hook)).next_u64()
    );
    assert!(w.terminal_plans.is_empty() && w.terminal_claims.is_empty());
    assert!(w.native_attempts.is_empty());
    w.fail_ready = false;
    assert!(a.tick(Frame::default(), &mut w).is_err());
    assert_eq!(w.ready_calls.borrow().len(), 1);
}

#[test]
fn xml_runtime_say_single_chance_miss_preflight_and_explicit_opportunity_intervals() {
    let def = compile(r#"<Behavior text="a;b;c" prob=".05">Say</Behavior>"#);
    let id = def.states[0].behaviors[0].local;
    let mut a = actor(def);
    let mut w = Recording::new();
    w.blocked.push(K::Say);
    tick(&mut a, &mut w, 0, 0);
    let mut reference = StdRng::seed_from_u64(local_seed(12345, id));
    assert_eq!(
        a.locals[&id].rng.clone().next_u64(),
        reference.clone().next_u64()
    );
    assert_eq!(a.locals[&id].opportunities, 0);
    w.blocked.clear();
    assert!(reference.r#gen::<f64>() > 0.05); // raw RNG oracle, independent of executor/chance implementation
    for at in (50..=500).step_by(50) {
        tick(&mut a, &mut w, at, 50);
    }
    assert_eq!(a.locals[&id].opportunities, 1);
    assert!(w.say_choices.is_empty());
    assert_eq!(a.locals[&id].rng.clone().next_u64(), reference.next_u64());
    let def = compile(
        r#"<Behavior bucket="movement" text="a;b;c" prob="0" cooldown=".1">Say</Behavior><Behavior bucket="movement">Wander</Behavior>"#,
    );
    let id = def.states[0].behaviors[0].local;
    let mut a = actor(def);
    let mut w = Recording::new();
    for at in [0, 50, 100, 150, 200] {
        tick(&mut a, &mut w, at, 50);
    }
    assert_eq!(a.locals[&id].opportunities, 3);
    assert_eq!(a.locals[&id].next, 300);
    assert_eq!(w.actions().len(), 5);
    assert!(
        w.actions()
            .iter()
            .all(|(_, s)| s.starts_with("apply:Wander"))
    );
    update(
        &mut a,
        &mut w,
        1200,
        Frame {
            stasis: true,
            delta_ms: 1000,
            ..Frame::default()
        },
    )
    .unwrap();
    update(
        &mut a,
        &mut w,
        2200,
        Frame {
            dormant: true,
            delta_ms: 1000,
            ..Frame::default()
        },
    )
    .unwrap();
    assert_eq!(a.locals[&id].opportunities, 3);
    tick(&mut a, &mut w, 2250, 50);
    tick(&mut a, &mut w, 2300, 50);
    assert_eq!(a.locals[&id].opportunities, 4);
}
#[test]
fn xml_runtime_say_uniform_choice_success_bucket_and_failure_no_replay() {
    let def = compile(
        r#"<Behavior bucket="movement" text="a;b;c" prob="1">Say</Behavior><Behavior bucket="movement">Wander</Behavior>"#,
    );
    let id = def.states[0].behaviors[0].local;
    let mut a = actor(def.clone());
    let mut w = Recording::new();
    let mut reference = StdRng::seed_from_u64(local_seed(12345, id));
    let choice = reference.gen_range(0..3);
    tick(&mut a, &mut w, 0, 0);
    assert_eq!(w.say_choices, vec![(0, choice)]);
    assert_eq!(w.actions().len(), 1);
    assert_eq!(a.locals[&id].rng.clone().next_u64(), reference.next_u64());
    tick(&mut a, &mut w, 50, 50);
    assert_eq!(w.say_choices.len(), 1);
    assert!(w.actions().last().unwrap().1.starts_with("apply:Wander"));
    let mut a = actor(def);
    let mut w = Recording::new();
    w.fail = true;
    assert!(a.tick(Frame::default(), &mut w).is_err());
    assert!(a.tick(Frame::default(), &mut w).is_err());
    assert!(w.say_choices.is_empty());
}
#[test]
fn xml_runtime_say_reentry_resets_but_active_ancestor_and_old_order_do_not() {
    let def = compile(
        r#"<Behavior text="root">Say</Behavior><State id="A"><Behavior text="child">Say</Behavior><Transition afterTime=".1">B</Transition></State><State id="B"><Transition afterTime=".1">A</Transition></State>"#,
    );
    let mut a = actor(def);
    let mut w = Recording::new();
    for at in [0, 50, 100, 150, 200, 250] {
        tick(&mut a, &mut w, at, 50);
    }
    assert_eq!(
        w.say_choices.iter().map(|(t, _)| *t).collect::<Vec<_>>(),
        vec![0, 0, 250]
    );
    let rules = authored(
        "Pirate",
        0x600,
        r#"<Order targetIds="Piratess"><Behavior text="order">Say</Behavior></Order>"#,
    ) + &authored(
        "Piratess",
        0x601,
        r#"<Behavior listenIds="Pirate">FollowOrders</Behavior>"#,
    );
    let mut game = compile_with(&CONTENT, rules, "Pirate");
    let issuer = Arc::new(game.definitions.remove(&0x600).unwrap());
    let order = &issuer.states[0].orders[0];
    let mut a = actor(Arc::new(game.definitions.remove(&0x601).unwrap()));
    let mut w = Recording::new();
    w.offers.push(offer(&issuer, order));
    tick(&mut a, &mut w, 0, 0);
    tick(&mut a, &mut w, 50, 50);
    assert_eq!(w.say_choices.len(), 1);
    w.offers[0].generation = 2;
    tick(&mut a, &mut w, 100, 50);
    tick(&mut a, &mut w, 3000, 2900);
    assert_eq!(
        w.say_choices.iter().map(|(t, _)| *t).collect::<Vec<_>>(),
        vec![0, 3000]
    );
}

#[test]
fn xml_runtime_repeated_order_listener_scopes_rng_and_death_hooks_are_independent() {
    let rules = authored(
        "Pirate",
        0x600,
        r#"<Order targetIds="Piratess"><Behavior effect="Invulnerable">ConditionEffect</Behavior><Behavior type="auto" cooldown=".1">Shoot</Behavior><Behavior childId="Pirate">ReleaseSpawnAtDeath</Behavior></Order>"#,
    ) + &authored(
        "Piratess",
        0x601,
        r#"<DefineEmbed name="Listener"><Behavior listenIds="Pirate">FollowOrders</Behavior></DefineEmbed><Embed name="Listener"/><Embed name="Listener"/>"#,
    );
    let mut game = compile_with(&CONTENT, rules, "Pirate");
    let issuer = Arc::new(game.definitions.remove(&0x600).unwrap());
    let order = &issuer.states[0].orders[0];
    let shot = issuer.states[order.state as usize]
        .behaviors
        .iter()
        .find(|b| b.kind == K::Shoot)
        .unwrap()
        .local;
    let follower = Arc::new(game.definitions.remove(&0x601).unwrap());
    let mut a = actor(follower.clone());
    let mut w = Recording::new();
    w.offers.push(offer(&issuer, order));
    tick(&mut a, &mut w, 0, 0);
    tick(&mut a, &mut w, 50, 50);
    tick(&mut a, &mut w, 100, 50);
    assert_eq!(
        w.actions()
            .iter()
            .filter(|(_, s)| s.starts_with("apply:Shoot"))
            .map(|(t, _)| *t)
            .collect::<Vec<_>>(),
        vec![0, 0, 100, 100]
    );
    let scopes: BTreeSet<_> = w
        .events
        .iter()
        .filter(|e| e.text.starts_with("apply:Shoot"))
        .map(|e| e.scope)
        .collect();
    assert_eq!(scopes.len(), 2);
    assert_eq!(
        scopes
            .iter()
            .filter_map(|scope| scope.listener)
            .collect::<BTreeSet<_>>()
            .len(),
        2
    );
    let draws: Vec<_> = a
        .locals
        .values()
        .filter_map(|local| local.listener.as_ref())
        .map(|listener| listener.locals[&shot].rng.clone().next_u64())
        .collect();
    assert_ne!(draws[0], draws[1]);
    a.defeated(&mut w).unwrap();
    a.defeated(&mut w).unwrap();
    assert_eq!(w.labels("death:").len(), 2);
    assert_eq!(w.labels("terminal:").len(), 1);
    assert_eq!(
        w.labels("condition:")
            .iter()
            .filter(|(_, s)| s.ends_with("true"))
            .count(),
        2
    );
    assert_eq!(
        w.labels("condition:")
            .iter()
            .filter(|(_, s)| s.ends_with("false"))
            .count(),
        2
    );
    let mut stale = Actor::new(follower, identity(11), 99).unwrap();
    let mut w2 = Recording::new();
    w2.offers = w.offers.clone();
    tick(&mut stale, &mut w2, 0, 0);
    w2.offers.clear();
    stale.defeated(&mut w2).unwrap();
    assert!(w2.labels("death:").is_empty());
    assert_eq!(w2.labels("terminal:").len(), 1);
}

#[test]
fn xml_runtime_random_docx_200ms_exact_trials_not_native_tick_or_hazard_conversion() {
    let source =
        std::fs::read_to_string("data/reference/release-7.0/BehaviorsAndTransitions.txt").unwrap();
    assert!(source.contains("every .2 seconds"));
    let def = compile(
        r#"<Behavior>RemainVisible</Behavior><State id="A"><Transition random=".000001">B</Transition></State><State id="B"/>"#,
    );
    let mut a = actor(def);
    let mut w = Recording::new();
    let mut reference = StdRng::seed_from_u64(12345);
    for (at, deadline) in [
        (0, 200),
        (50, 200),
        (100, 200),
        (150, 200),
        (200, 400),
        (400, 600),
    ] {
        tick(&mut a, &mut w, at, if at == 400 { 200 } else { 50 });
        assert_eq!(a.trials[&(1, 0)], deadline);
        if [0, 200, 400].contains(&at) {
            assert!(reference.r#gen::<f64>() > 0.000001);
        }
        assert_eq!(a.rng.clone().next_u64(), reference.clone().next_u64());
        assert_eq!(a.path().last(), Some(&1));
    }
    receipt(
        "xml-runtime-random-cadence.json",
        serde_json::json!({"source_basis":"DOCX271 probability per .2seconds; immediate-first and activation/eligibility clocks authored", "native_step_ms":50,"chance_trial_active_ms":[0,200,400],"literal_probability":0.000001,"no_hazard_conversion":true}),
    );
}
#[test]
fn xml_runtime_random_docx_prerequisites_before_draw_and_sequential_later_transition() {
    let def = compile(
        r#"<Behavior>RemainVisible</Behavior><State id="A"><Transition random=".000001" playerWithin="2">B</Transition></State><State id="B"/>"#,
    );
    let mut a = actor(def);
    let mut w = Recording::new();
    let mut reference = a.rng.clone();
    tick(&mut a, &mut w, 0, 0);
    assert!(a.trials.is_empty());
    assert_eq!(a.rng.clone().next_u64(), reference.clone().next_u64());
    w.players = vec![2.];
    tick(&mut a, &mut w, 50, 50);
    assert_eq!(a.trials[&(1, 0)], 250);
    assert!(reference.r#gen::<f64>() > 0.000001);
    for at in [100, 150, 200] {
        tick(&mut a, &mut w, at, 50);
        assert_eq!(a.rng.clone().next_u64(), reference.clone().next_u64());
    }
    tick(&mut a, &mut w, 250, 50);
    assert_eq!(a.trials[&(1, 0)], 450);
    assert!(reference.r#gen::<f64>() > 0.000001);
    assert_eq!(a.rng.clone().next_u64(), reference.next_u64());
    let def = compile(
        r#"<Behavior>RemainVisible</Behavior><State id="A"><Transition random="0">B</Transition><Transition random="1">C</Transition></State><State id="B"/><State id="C"/>"#,
    );
    let mut a = actor(def);
    let mut w = Recording::new();
    let mut reference = a.rng.clone();
    tick(&mut a, &mut w, 0, 0);
    assert_eq!(
        a.definition.states[*a.path.last().unwrap() as usize].name,
        "C"
    );
    assert_eq!(a.rng.next_u64(), reference.next_u64());
}
#[test]
fn xml_runtime_random_docx_activation_reset_and_ancestor_trial_persistence() {
    let def = compile(
        r#"<Behavior>RemainVisible</Behavior><State id="Active"><Transition random="0">Done</Transition><State id="First"><Transition random="0">Second</Transition><Transition afterTime=".1">Second</Transition></State><State id="Second"><Transition random="0">First</Transition><Transition afterTime=".1">First</Transition></State></State><State id="Done"/>"#,
    );
    let mut a = actor(def);
    let mut w = Recording::new();
    for at in [0, 50, 100, 150, 200, 250, 300, 350, 400] {
        tick(&mut a, &mut w, at, 50);
    }
    assert_eq!(a.trials[&(1, 0)], 600);
    assert!(!a.trials.contains_key(&(2, 0)));
    assert!(!a.trials.contains_key(&(3, 0)));
    tick(&mut a, &mut w, 450, 50);
    assert_eq!(a.trials[&(1, 0)], 600);
    assert_eq!(a.trials[&(2, 0)], 650);
}
#[test]
fn xml_runtime_random_docx_stasis_dormancy_and_longstep_have_no_extra_trials() {
    let def = compile(
        r#"<Behavior>RemainVisible</Behavior><State id="A"><Transition random=".000001">B</Transition></State><State id="B"/>"#,
    );
    let mut a = actor(def);
    let mut w = Recording::new();
    let mut reference = StdRng::seed_from_u64(12345);
    tick(&mut a, &mut w, 0, 0);
    assert!(reference.r#gen::<f64>() > 0.000001);
    update(
        &mut a,
        &mut w,
        1000,
        Frame {
            stasis: true,
            delta_ms: 1000,
            ..Frame::default()
        },
    )
    .unwrap();
    update(
        &mut a,
        &mut w,
        2000,
        Frame {
            dormant: true,
            delta_ms: 1000,
            ..Frame::default()
        },
    )
    .unwrap();
    assert_eq!(a.active_ms(), 0);
    assert_eq!(a.trials[&(1, 0)], 200);
    tick(&mut a, &mut w, 2050, 50);
    tick(&mut a, &mut w, 2200, 150);
    assert!(reference.r#gen::<f64>() > 0.000001);
    assert_eq!(a.trials[&(1, 0)], 400);
    tick(&mut a, &mut w, 5000, 2800);
    assert!(reference.r#gen::<f64>() > 0.000001);
    assert_eq!(a.trials[&(1, 0)], 3200);
    assert_eq!(a.rng.clone().next_u64(), reference.next_u64());
}

const ANNOUNCEMENT_CATEGORIES: &str = r#"<Strings id="new"><String>Born A</String><String>Born B</String></Strings><Strings id="killed"><String>Defeated by {KILLER} A</String><String>{KILLER} won B</String></Strings><Strings id="death"><String>Gone A</String><String>Gone B</String></Strings>"#;
fn announcement_inputs(rules: &str, lists: &str, root: &str) -> BTreeMap<String, Vec<u8>> {
    crate::content::game::test_inputs::selected([
        ("game/123.5.1/game.xml",format!(r#"<Game schema="1"><Include file="rules.xml"/><Include file="strings.xml"/><Root object="{root}"/></Game>"#).into_bytes()),
        ("game/123.5.1/rules.xml",fixture_rules(rules)),
        ("game/123.5.1/strings.xml",format!("<StringLists>{lists}</StringLists>").into_bytes()),
    ])
}
fn announcement_game(rules: &str, lists: &str) -> Result<GameData> {
    game::compile_fixture(&CONTENT, announcement_inputs(rules, lists, "Pirate"))
}
fn announcer(body: &str, categories: &str) -> Arc<Definition> {
    let rules = authored("Pirate", 0x600, body)
        + &authored("Piratess", 0x601, "<Behavior>RemainVisible</Behavior>");
    let lists = format!(r#"<StringList id="Pirate">{categories}</StringList>"#);
    Arc::new(
        announcement_game(&rules, &lists)
            .unwrap()
            .definitions
            .remove(&0x600)
            .unwrap(),
    )
}
fn player_kill() -> TerminalContext {
    TerminalContext::PlayerKill {
        world: 1,
        player: 55,
        name: "Ada".into(),
    }
}
fn template_text(parts: &[TextPart]) -> String {
    parts
        .iter()
        .map(|part| match part {
            TextPart::Literal(s) => s.as_str(),
            TextPart::Killer => "{KILLER}",
            TextPart::Player => "{PLAYER}",
            TextPart::Count => "{COUNT}",
            TextPart::Hitpoints => "{HITPOINTS}",
        })
        .collect()
}
#[test]
fn xml_compile_oryx_actual_eight_implicit_links_source_categories_and_birth_only() {
    use roxmltree::Node;
    fn signature(node: Node<'_, '_>) -> serde_json::Value {
        serde_json::json!({"tag":node.tag_name().name(),"attributes":node.attributes().map(|a|(a.name(),a.value())).collect::<BTreeMap<_,_>>(),
            "text":if node.has_tag_name("String"){node.text()}else{None},
            "children":node.children().filter(|n|n.is_element()).map(signature).collect::<Vec<_>>()})
    }
    let selected =
        roxmltree::Document::parse(include_str!("../../data/game/123.5.1/strings.xml")).unwrap();
    let mut expected_lists = BTreeSet::from([
        "Lord of the Lost Lands",
        "Grand Sphinx",
        "Skull Shrine",
        "Cube God",
        "Pentaract",
        "Hermit God",
        "Ghost Ship",
        "Beach Bum",
    ]);
    let source = std::fs::read_to_string("data/reference/release-7.0/xml/stringlist.xml").unwrap();
    let doc = roxmltree::Document::parse(&source).unwrap();
    let mut observations = Vec::new();
    let mut birth_only = 0;
    for definition in PRODUCTION.definitions.values() {
        for b in definition
            .states
            .iter()
            .flat_map(|state| &state.behaviors)
            .filter(|b| b.kind == K::OryxTaunt)
        {
            let rules = b
                .announcement
                .as_ref()
                .expect("implicit link required before startup");
            let name = &CONTENT.objects[&definition.kind].name;
            let list = &PRODUCTION.string_lists[rules.list as usize];
            assert_eq!(&list.name, name);
            let matches: Vec<_> = doc
                .descendants()
                .filter(|n| {
                    n.has_tag_name("StringList") && n.attribute("id") == Some(name.as_str())
                })
                .collect();
            assert_eq!(matches.len(), 1);
            assert!(
                expected_lists.remove(name.as_str()),
                "unexpected list {name}"
            );
            let chosen: Vec<_> = selected
                .root_element()
                .children()
                .filter(|n| n.has_tag_name("StringList") && n.attribute("id") == Some(name))
                .collect();
            assert_eq!(chosen.len(), 1, "selected exact list {name}");
            assert_eq!(
                signature(matches[0]),
                signature(chosen[0]),
                "source tree {name}"
            );
            let expected: BTreeMap<_, Vec<String>> = matches[0]
                .children()
                .filter(|n| n.is_element())
                .map(|category| {
                    (
                        category.attribute("id").unwrap().to_string(),
                        category
                            .children()
                            .filter(|n| n.is_element())
                            .map(|message| message.text().unwrap().trim().to_string())
                            .collect(),
                    )
                })
                .collect();
            assert_eq!(
                rules
                    .admission
                    .iter()
                    .map(|parts| template_text(parts))
                    .collect::<Vec<_>>(),
                expected["new"]
            );
            assert_eq!(rules.source, list.source);
            assert!(rules.source.line > 0);
            assert!(
                !b.fields.contains_key(&F::ListId),
                "no invented source listId attribute"
            );
            match &rules.terminal {
                TerminalAnnouncements::BirthOnly => {
                    birth_only += 1;
                    assert!(!expected.contains_key("killed") && !expected.contains_key("death"));
                    assert_eq!(name, "Beach Bum");
                }
                TerminalAnnouncements::Death { killed, death } => {
                    assert_eq!(
                        killed
                            .iter()
                            .map(|parts| template_text(parts))
                            .collect::<Vec<_>>(),
                        expected["killed"]
                    );
                    assert_eq!(
                        death
                            .iter()
                            .map(|parts| template_text(parts))
                            .collect::<Vec<_>>(),
                        expected["death"]
                    );
                }
            }
            for (category, texts) in &list.categories {
                assert_eq!(
                    texts
                        .iter()
                        .map(|parts| template_text(parts))
                        .collect::<Vec<_>>(),
                    expected[category]
                );
            }
            let mut controller = actor(Arc::new(definition.clone()));
            let mut world = Recording::new();
            tick(&mut controller, &mut world, 0, 0);
            tick(&mut controller, &mut world, 50, 50);
            assert_eq!(
                world.admission_attempts.len(),
                1,
                "{name} constructor is not periodic"
            );
            let admitted = &world.admission_attempts[0];
            assert_eq!(admitted.text, expected["new"][admitted.choice]);
            controller
                .defeated_with_context(player_kill(), &mut world)
                .unwrap();
            if matches!(rules.terminal, TerminalAnnouncements::BirthOnly) {
                assert!(world.terminal_announcements.is_empty());
            } else {
                assert_eq!(world.terminal_announcements.len(), 1);
                let terminal = &world.terminal_announcements[0];
                assert_eq!(terminal.key.category, AnnouncementCategory::Killed);
                assert_eq!(
                    terminal.text,
                    expected["killed"][terminal.choice].replace("{KILLER}", "Ada")
                );
            }
            observations.push(serde_json::json!({"object":name,"kind":definition.kind,"list":rules.list,"source":rules.source,"birth_only":matches!(rules.terminal,TerminalAnnouncements::BirthOnly),"categories":expected,"mock_admission":world.admission_attempts.iter().map(|a|format!("{a:?}")).collect::<Vec<_>>(),"mock_terminal":world.terminal_announcements.iter().map(|a|format!("{a:?}")).collect::<Vec<_>>()}));
        }
    }
    assert!(expected_lists.is_empty());
    let lists = PRODUCTION
        .string_lists
        .iter()
        .filter(|list| observations.iter().any(|row| row["object"] == list.name));
    assert_eq!(
        lists
            .clone()
            .map(|list| list.categories.len())
            .sum::<usize>(),
        35
    );
    assert_eq!(
        lists
            .flat_map(|list| list.categories.values())
            .map(Vec::len)
            .sum::<usize>(),
        95
    );
    assert_eq!(observations.len(), 8);
    assert_eq!(birth_only, 1);
    // Seventeen OryxTaunt/Taunt lists plus the Ent Ancient and Ghost King remaining-quest lists.
    assert_eq!(PRODUCTION.string_lists.len(), 19);
    assert_eq!(
        PRODUCTION
            .string_lists
            .iter()
            .map(|list| list.categories.len())
            .sum::<usize>(),
        64
    );
    assert_eq!(
        PRODUCTION
            .string_lists
            .iter()
            .flat_map(|list| list.categories.values())
            .map(Vec::len)
            .sum::<usize>(),
        190
    );
    receipt(
        "xml-oryx-production-links.json",
        serde_json::json!({"qualification":"actual selected-source -> restored data -> implicit typed compiler links; not native publication/history acceptance","input_fingerprint":PRODUCTION.fingerprint,"announcers":observations}),
    );
}
#[test]
fn xml_compile_oryx_missing_ambiguous_new_and_partial_death_refuse_contextually() {
    let rules = authored(
        "Pirate",
        0x600,
        r#"<DefineEmbed name="Voice"><Behavior>OryxTaunt</Behavior></DefineEmbed><State id="Active"><Embed name="Voice"/></State>"#,
    );
    let cases=[
        ("<StringList id=\"Piratess\"><Strings id=\"new\"><String>Other actor</String></Strings></StringList>".to_string(),"implicit exact selected-object-name StringList"),
        (format!(r#"<StringList id="Pirate">{ANNOUNCEMENT_CATEGORIES}</StringList><StringList id="Pirate">{ANNOUNCEMENT_CATEGORIES}</StringList>"#),"duplicate StringList Pirate"),
        (r#"<StringList id="Pirate"><Strings id="death"><String>Gone</String></Strings><Strings id="killed"><String>{KILLER}</String></Strings></StringList>"#.into(),"missing required new category"),
        (r#"<StringList id="Pirate"><Strings id="new"><String>Born</String></Strings><Strings id="killed"><String>{KILLER}</String></Strings></StringList>"#.into(),"both killed and death"),
        (r#"<StringList id="Pirate"><Strings id="new"><String>{KILLER}</String></Strings></StringList>"#.into(),"unbound announcement token"),
    ];
    for (lists, expected) in cases {
        let error = format!("{:#}", announcement_game(&rules, &lists).unwrap_err());
        assert!(error.contains(expected), "{error}");
        assert!(
            error.contains("strings.xml") || error.contains("rules.xml"),
            "{error}"
        );
        if !expected.starts_with("duplicate") {
            assert!(
                error.contains("object=Pirate")
                    && error.contains("state=@root/Active")
                    && error.contains("Voice"),
                "{error}"
            );
        }
    }
}
#[test]
fn xml_compile_oryx_actual_production_missing_implicit_list_is_not_fallback() {
    let mut inputs: BTreeMap<_, _> = PRODUCTION
        .inputs
        .keys()
        .map(|path| {
            (
                path.clone(),
                std::fs::read(Path::new("data").join(path)).unwrap(),
            )
        })
        .collect();
    let key = "game/123.5.1/strings.xml";
    let text = String::from_utf8(inputs[key].clone()).unwrap();
    let doc = roxmltree::Document::parse(&text).unwrap();
    let b = PRODUCTION
        .definitions
        .values()
        .flat_map(|d| d.states.iter().flat_map(|s| &s.behaviors))
        .find(|b| {
            b.announcement
                .as_ref()
                .is_some_and(|rules| matches!(rules.terminal, TerminalAnnouncements::BirthOnly))
        })
        .unwrap();
    let list = &PRODUCTION.string_lists[b.announcement.as_ref().unwrap().list as usize];
    let node = doc
        .descendants()
        .find(|n| n.has_tag_name("StringList") && n.attribute("id") == Some(list.name.as_str()))
        .unwrap();
    let mut changed = text.clone();
    changed.replace_range(node.range(), "");
    inputs.insert(key.into(), changed.into_bytes());
    let error = format!(
        "{:#}",
        game::compile_fixture_at(&CONTENT, inputs, Path::new("data")).unwrap_err()
    );
    assert!(
        error.contains("OryxTaunt implicit")
            && error.contains(&list.name)
            && error.contains("actors.xml")
            && error.contains("state=@root"),
        "{error}"
    );
    receipt(
        "xml-oryx-production-negative.json",
        serde_json::json!({"removed_exact_source_list":list.name,"error":error,"qualification":"actual production inputs mutated only in memory; compiler refusal, not startup/native acceptance"}),
    );
}
#[test]
fn xml_runtime_oryx_admission_once_per_activation_not_every_update_or_dormancy() {
    let mut a = actor(announcer(
        "<Behavior>OryxTaunt</Behavior>",
        ANNOUNCEMENT_CATEGORIES,
    ));
    let mut w = Recording::new();
    update(
        &mut a,
        &mut w,
        0,
        Frame {
            dormant: true,
            ..Frame::default()
        },
    )
    .unwrap();
    for at in [50, 100, 200, 1000] {
        tick(&mut a, &mut w, at, 50);
    }
    assert_eq!(w.admission_claims.len(), 1);
    assert_eq!(w.admission_attempts.len(), 1);
    assert!(w.actions().is_empty());
    assert_eq!(
        w.admission_attempts[0].key.category,
        AnnouncementCategory::New
    );
    assert_eq!(
        w.admission_attempts[0].key.owner.scope.identity,
        identity(10)
    );
    assert!(w.terminal_announcements.is_empty());
}
#[test]
fn xml_runtime_oryx_admission_rejected_and_false_failure_retry_cached_choice_no_double_publish() {
    for after_claim in [false, true] {
        let mut a = actor(announcer(
            "<Behavior>OryxTaunt</Behavior>",
            ANNOUNCEMENT_CATEGORIES,
        ));
        let mut w = Recording::new();
        w.fail_admission = !after_claim;
        w.fail_admission_after_claim = after_claim;
        assert!(a.tick(Frame::default(), &mut w).is_err());
        let pending = a.admissions.values().next().unwrap().clone();
        let local = pending.key.owner.local;
        let mut rng = a.locals[&local].rng.clone();
        assert_eq!(w.admission_claims.len(), usize::from(after_claim));
        assert!(a.tick(Frame::default(), &mut w).is_err());
        w.fail_admission = false;
        w.fail_admission_after_claim = false;
        a.retry_admissions(&mut w).unwrap();
        a.retry_admissions(&mut w).unwrap();
        assert_eq!(w.admission_attempts, vec![pending.clone(), pending]);
        assert_eq!(w.admission_claims.len(), 1);
        assert_eq!(a.locals[&local].rng.clone().next_u64(), rng.next_u64());
        tick(&mut a, &mut w, 1000, 50);
        assert_eq!(w.admission_attempts.len(), 2);
    }
    let rules = authored(
        "Pirate",
        0x600,
        r#"<Order targetIds="Piratess"><Behavior>OryxTaunt</Behavior></Order>"#,
    ) + &authored(
        "Piratess",
        0x601,
        r#"<Behavior listenIds="Pirate">FollowOrders</Behavior>"#,
    );
    let lists = format!(r#"<StringList id="Pirate">{ANNOUNCEMENT_CATEGORIES}</StringList>"#);
    let mut game = announcement_game(&rules, &lists).unwrap();
    let issuer = Arc::new(game.definitions.remove(&0x600).unwrap());
    let order = &issuer.states[0].orders[0];
    let mut a = actor(Arc::new(game.definitions.remove(&0x601).unwrap()));
    let mut w = Recording::new();
    w.offers.push(offer(&issuer, order));
    w.fail_admission_after_claim = true;
    assert!(a.tick(Frame::default(), &mut w).is_err());
    assert!(a.admission_failed && a.failed);
    assert_eq!(w.admission_claims.len(), 1);
    let sampled = w.admission_attempts[0].clone();
    w.fail_admission_after_claim = false;
    a.retry_admissions(&mut w).unwrap();
    tick(&mut a, &mut w, 50, 50);
    assert_eq!(w.admission_attempts, vec![sampled.clone(), sampled]);
    assert_eq!(w.admission_claims.len(), 1);
}
#[test]
fn xml_runtime_oryx_terminal_context_category_tokens_and_retry_single_atomic_claim() {
    for (context, category) in [
        (player_kill(), AnnouncementCategory::Killed),
        (TerminalContext::NonPlayer, AnnouncementCategory::Death),
    ] {
        let mut a = actor(announcer(
            "<Behavior>OryxTaunt</Behavior><Behavior>VictoryAtDeath</Behavior>",
            ANNOUNCEMENT_CATEGORIES,
        ));
        let mut w = Recording::new();
        tick(&mut a, &mut w, 0, 0);
        assert!(a.defeated(&mut w).is_err());
        assert!(w.terminal_attempts.is_empty());
        assert!(!a.failed);
        w.fail_terminal = true;
        assert!(a.defeated_with_context(context.clone(), &mut w).is_err());
        let plan = a.pending.clone().unwrap();
        assert_eq!(plan.context, Some(context.clone()));
        assert_eq!(plan.announcements.len(), 1);
        assert_eq!(plan.announcements[0].key.category, category);
        let text = &plan.announcements[0].text;
        if category == AnnouncementCategory::Killed {
            assert!(text.contains("Ada") && !text.contains("{KILLER}"));
        } else {
            assert!(text == "Gone A" || text == "Gone B");
        }
        let local = plan.announcements[0].key.owner.local;
        let mut rng = a.locals[&local].rng.clone();
        assert!(w.terminal_announcements.is_empty());
        assert!(w.labels("death:").is_empty());
        assert!(a.retry_terminal(&mut w).is_err());
        assert_eq!(a.locals[&local].rng.clone().next_u64(), rng.next_u64());
        w.fail_terminal = false;
        a.retry_terminal(&mut w).unwrap();
        a.defeated_with_context(context, &mut w).unwrap();
        assert_eq!(w.terminal_announcements, plan.announcements);
        assert_eq!(w.labels("death:").len(), 1);
        assert_eq!(w.labels("terminal:").len(), 1);
        assert!(
            w.terminal_plans
                .iter()
                .all(|attempt| format!("{attempt:?}") == format!("{plan:?}"))
        );
    }
}
#[test]
fn xml_runtime_oryx_invalid_killer_context_precedes_all_effects_and_rng() {
    for context in [
        TerminalContext::PlayerKill {
            world: 2,
            player: 55,
            name: "Ada".into(),
        },
        TerminalContext::PlayerKill {
            world: 1,
            player: 0,
            name: "Ada".into(),
        },
        TerminalContext::PlayerKill {
            world: 1,
            player: 55,
            name: " ".into(),
        },
    ] {
        let mut a = actor(announcer(
            "<Behavior>OryxTaunt</Behavior>",
            ANNOUNCEMENT_CATEGORIES,
        ));
        let mut w = Recording::new();
        let mut rng = a.rng.clone();
        assert!(a.defeated_with_context(context, &mut w).is_err());
        assert!(w.events.is_empty());
        assert!(!a.started);
        assert_eq!(a.rng.next_u64(), rng.next_u64());
    }
    let mut a = actor(announcer(
        "<Behavior>OryxTaunt</Behavior>",
        ANNOUNCEMENT_CATEGORIES,
    ));
    let mut w = Recording::new();
    assert!(a.defeated(&mut w).is_err());
    assert!(w.events.is_empty());
}
#[test]
fn xml_runtime_oryx_birth_only_transform_and_nondeath_cleanup_do_not_invent_death_prose() {
    for body in [
        "<Behavior>OryxTaunt</Behavior>",
        "<Behavior>OryxTaunt</Behavior><Behavior time=\"0\" childId=\"Piratess\">Metamorphose</Behavior>",
    ] {
        let mut a = actor(announcer(
            body,
            r#"<Strings id="new"><String>Only born</String></Strings>"#,
        ));
        let mut w = Recording::new();
        tick(&mut a, &mut w, 0, 0);
        if !a.retired() {
            a.defeated(&mut w).unwrap();
        }
        assert_eq!(w.admission_claims.len(), 1);
        assert!(w.terminal_announcements.is_empty());
    }
    for operation in [
        r#"<Behavior time="0">Decay</Behavior>"#,
        r#"<Behavior numShots="0">Explode</Behavior>"#,
    ] {
        let mut a = actor(announcer(
            &(String::from("<Behavior>OryxTaunt</Behavior>") + operation),
            ANNOUNCEMENT_CATEGORIES,
        ));
        let mut w = Recording::new();
        tick(&mut a, &mut w, 0, 0);
        assert_eq!(
            w.terminal_plans[0].context,
            Some(TerminalContext::NonPlayer)
        );
        assert_eq!(w.terminal_announcements.len(), 1);
        assert_eq!(
            w.terminal_announcements[0].key.category,
            AnnouncementCategory::Death
        );
    }
    let mut a = actor(announcer(
        "<Behavior>OryxTaunt</Behavior>",
        ANNOUNCEMENT_CATEGORIES,
    ));
    let mut w = Recording::new();
    tick(&mut a, &mut w, 0, 0);
    a.cleanup(&mut w).unwrap();
    assert!(w.terminal_plans.is_empty());
    let mut a = actor(announcer(
        "<Behavior>OryxTaunt</Behavior><Behavior time=\"0\" childId=\"Piratess\">Metamorphose</Behavior>",
        ANNOUNCEMENT_CATEGORIES,
    ));
    let mut w = Recording::new();
    tick(&mut a, &mut w, 0, 0);
    assert_eq!(w.terminal_plans[0].reason, TerminalReason::Metamorphose);
    assert!(w.terminal_announcements.is_empty());
}
#[test]
fn xml_runtime_oryx_state_activation_keys_ancestor_persistence_and_active_only_death() {
    let body = r#"<State id="Active"><Behavior>OryxTaunt</Behavior><State id="First"><Behavior>OryxTaunt</Behavior><Transition afterTime=".1">Second</Transition></State><State id="Second"><Transition afterTime=".1">First</Transition></State><Transition afterTime=".4">Done</Transition></State><State id="Done"/>"#;
    let def = announcer(body, ANNOUNCEMENT_CATEGORIES);
    let mut a = actor(def.clone());
    let mut w = Recording::new();
    for at in [0, 50, 100, 150, 200, 250, 300, 350, 400] {
        tick(&mut a, &mut w, at, 50);
    }
    assert_eq!(
        w.labels("announcement:New")
            .iter()
            .map(|(time, _)| *time)
            .collect::<Vec<_>>(),
        vec![0, 0, 200]
    );
    assert_eq!(w.admission_claims.len(), 3);
    a.defeated(&mut w).unwrap();
    assert!(w.terminal_announcements.is_empty());
    let mut a = actor(def);
    let mut w = Recording::new();
    for at in [0, 100, 200] {
        tick(&mut a, &mut w, at, 100);
    }
    a.defeated_with_context(TerminalContext::NonPlayer, &mut w)
        .unwrap();
    assert_eq!(w.terminal_announcements.len(), 2);
}
#[test]
fn xml_runtime_oryx_order_activation_and_terminal_merge_world_scoped_false_failure() {
    let rules = authored(
        "Pirate",
        0x600,
        r#"<Order targetIds="Piratess"><Behavior>OryxTaunt</Behavior></Order>"#,
    ) + &authored(
        "Piratess",
        0x601,
        r#"<Behavior listenIds="Pirate">FollowOrders</Behavior><Behavior>VictoryAtDeath</Behavior>"#,
    );
    let lists = format!(r#"<StringList id="Pirate">{ANNOUNCEMENT_CATEGORIES}</StringList>"#);
    let mut game = announcement_game(&rules, &lists).unwrap();
    let issuer = Arc::new(game.definitions.remove(&0x600).unwrap());
    let order = &issuer.states[0].orders[0];
    let follower = Arc::new(game.definitions.remove(&0x601).unwrap());
    let mut a = actor(follower.clone());
    let mut w = Recording::new();
    w.offers.push(offer(&issuer, order));
    tick(&mut a, &mut w, 0, 0);
    tick(&mut a, &mut w, 50, 50);
    assert_eq!(w.admission_claims.len(), 1);
    w.offers[0].generation = 2;
    tick(&mut a, &mut w, 100, 50);
    assert_eq!(w.admission_claims.len(), 1);
    tick(&mut a, &mut w, 3000, 2900);
    assert_eq!(w.admission_claims.len(), 2);
    assert_eq!(
        w.admission_claims
            .keys()
            .map(|key| key.owner.scope.generation)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([1, 2])
    );
    w.fail_terminal_after_claim = true;
    assert!(a.defeated_with_context(player_kill(), &mut w).is_err());
    let plan = a.pending.clone().unwrap();
    assert_eq!(plan.announcements.len(), 1);
    assert!(plan.announcements[0].key.owner.scope.order.is_some());
    assert_eq!(w.terminal_announcements.len(), 1);
    assert_eq!(w.labels("death:").len(), 1);
    w.fail_terminal_after_claim = false;
    a.retry_terminal(&mut w).unwrap();
    assert_eq!(w.terminal_announcements.len(), 1);
    assert_eq!(w.terminal_plans.len(), 2);
    assert!(
        w.terminal_plans
            .iter()
            .all(|p| format!("{p:?}") == format!("{plan:?}"))
    );
    let mut other = Actor::new(
        announcer("<Behavior>OryxTaunt</Behavior>", ANNOUNCEMENT_CATEGORIES),
        Identity {
            world: 2,
            ..identity(10)
        },
        12345,
    )
    .unwrap();
    tick(&mut other, &mut w, 0, 0);
    assert_eq!(
        w.admission_claims
            .keys()
            .map(|key| key.owner.scope.identity.world)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([1, 2])
    );
    let mut stale = Actor::new(follower, identity(11), 12345).unwrap();
    tick(&mut stale, &mut w, 3100, 0);
    w.offers.clear();
    stale.defeated(&mut w).unwrap();
    assert!(w.terminal_plans.last().unwrap().announcements.is_empty());
}
