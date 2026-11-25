use anyhow::{Result, bail};
use serde::Serialize;

macro_rules! fields {
    ($($variant:ident => $spelling:literal),* $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
        pub enum Field { $($variant),* }
        impl Field {
            pub fn parse(name: &str) -> Result<Self> {
                Ok(match name {
                    $($spelling => Self::$variant,)*
                    _ => bail!("unknown server attribute {name}; spelling corrections must be explicit in runtime XML"),
                })
            }
        }
    };
}
macro_rules! behaviors {
    ($($variant:ident => [$($field:ident),* $(,)?]),* $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
        pub enum BehaviorKind { $($variant),* }
        impl BehaviorKind {
            #[cfg(test)]
            pub const ALL: &[Self] = &[$(Self::$variant),*];
            pub fn parse(name: &str) -> Result<Self> {
                Ok(match name {
                    $(stringify!($variant) => Self::$variant,)*
                    _ => bail!("unknown server behavior {name}"),
                })
            }
            pub fn fields(self) -> &'static [Field] {
                match self { $(Self::$variant => &[$(Field::$field),*],)* }
            }
        }
    };
}
fields! {
    Acceleration => "acceleration",
    AcquireRange => "acquireRange",
    AltTextureId => "altTextureId",
    Angle => "angle",
    ArcDegrees => "arcDegrees",
    AvoidWater => "avoidWater",
    BlastEffect => "blastEffect",
    Broadcast => "broadcast",
    BubbleTime => "bubbleTime",
    CenterObject => "centerObject",
    ChildCorpseId => "childCorpseId",
    ChildId => "childId",
    ChildIds => "childIds",
    Clockwise => "clockwise",
    Color => "color",
    ConditionDuration => "conditionDuration",
    ConditionEffect => "conditionEffect",
    Cooldown => "cooldown",
    CooldownJitter => "cooldownJitter",
    Damage => "damage",
    DefaultAngle => "defaultAngle",
    Distance => "distance",
    DropLoot => "dropLoot",
    Duration => "duration",
    Effect => "effect",
    FlashPeriod => "flashPeriod",
    FlashRepeats => "flashRepeats",
    GiveXP => "giveXP",
    GroundId => "groundId",
    GroundTypes => "groundTypes",
    GroupId => "groupId",
    GroupIds => "groupIds",
    Healing => "healing",
    HideVisual => "hideVisual",
    IgnoreInvisible => "ignoreInvisible",
    InitialDelay => "initialDelay",
    ListId => "listId",
    ListenIds => "listenIds",
    Map => "map",
    Max => "max",
    MaxChildren => "maxChildren",
    MaxDamage => "maxDamage",
    MaxHeal => "maxHeal",
    MaxPlace => "maxPlace",
    MaxSize => "maxSize",
    MaxTargets => "maxTargets",
    MaxAltitude => "maxAltitude",
    Min => "min",
    MinAltitude => "minAltitude",
    MinDamage => "minDamage",
    MinRange => "minRange",
    MinSize => "minSize",
    NoCooldownJitter => "noCooldownJitter",
    NumChildren => "numChildren",
    NumShots => "numShots",
    ObjectId => "objectId",
    Offset => "offset",
    ParticleEffect => "particleEffect",
    PoisonDuration => "poisonDuration",
    PoisonTotalDamage => "poisonTotalDamage",
    PosJitter => "posJitter",
    Predictive => "predictive",
    Prob => "prob",
    ProjectileId => "projectileId",
    ProtecteeId => "protecteeId",
    ProtectionRange => "protectionRange",
    Radius => "radius",
    RadiusVariability => "radiusVariability",
    RadiusWobbleDistance => "radiusWobbleDistance",
    RadiusWobblePeriod => "radiusWobblePeriod",
    Range => "range",
    RangeJitter => "rangeJitter",
    Rate => "rate",
    ReprotectRange => "reprotectRange",
    ReshadowRange => "reshadowRange",
    ResignPet => "resignPet",
    SendNotification => "sendNotification",
    SetGlobalTag => "setGlobalTag",
    SoundId => "soundId",
    Speed => "speed",
    SpeedVariability => "speedVariability",
    StartWithMinions => "startWithMinions",
    TagName => "tagName",
    TargetGroups => "targetGroups",
    TargetId => "targetId",
    TargetIds => "targetIds",
    Terrain => "terrain",
    Text => "text",
    TextureSourceId => "textureSourceId",
    TilDeath => "tilDeath",
    Time => "time",
    TurnRate => "turnRate",
    Type => "type",
}
behaviors! {
    BackAndForth => [Radius, Speed, Angle, TurnRate],
    Buzz => [Acceleration, AcquireRange, Cooldown, Range, Speed, TurnRate],
    ChangeGround => [GroundId],
    ChangeSize => [MaxSize, MinSize, Rate],
    Charge => [Cooldown, Range, Speed],
    Circle => [AcquireRange, Cooldown, Distance, Range, Speed],
    ClearConditionEffectAura => [Cooldown, Effect, IgnoreInvisible, Range],
    ClearGlobalTag => [TagName],
    ClearTag => [TagName],
    ConditionEffect => [Cooldown, Duration, Effect],
    ConditionEffectAura => [Cooldown, Effect, Range],
    CopyDamageRecord => [TargetId],
    Decay => [Color, DropLoot, GiveXP, ParticleEffect, ResignPet, TilDeath, Time],
    DeleteObject => [ObjectId, Radius],
    Explode => [ArcDegrees, DropLoot, GiveXP, InitialDelay, NumShots, ProjectileId, Range, Type],
    Follow => [AcquireRange, Cooldown, Distance, Duration, Predictive, Range, Speed],
    FollowOrders => [AcquireRange, Cooldown, GroupIds, ListenIds],
    GrenadeToss => [Cooldown, Damage, DefaultAngle, Duration, Effect, MinRange, Radius, Range, TextureSourceId, Type],
    Heal => [Cooldown, HideVisual, MaxHeal, MaxTargets, Range, TargetGroups, TargetIds],
    HealZap => [Color, Cooldown, Healing, Range],
    KeepDistance => [AcquireRange, Distance, Range, Speed],
    MaintainProtectors => [ChildCorpseId, ChildIds, Cooldown, Distance, NumChildren],
    MakeMinions => [ChildIds, Cooldown, GroupId, MaxChildren, Offset, StartWithMinions],
    Metamorphose => [ChildId, Time, DropLoot, GiveXP],
    MetamorphoseAtDeath => [ChildId, PosJitter, Prob],
    Move => [Angle, Cooldown, Distance, Speed],
    ObjectToss => [CenterObject, ChildId, Cooldown, Damage, DefaultAngle, MinRange, Offset, Radius, Range, RangeJitter, TextureSourceId, Type],
    Orbit => [AcquireRange, Clockwise, ProtecteeId, ProtectionRange, Radius, RadiusVariability, RadiusWobbleDistance, RadiusWobblePeriod, ReprotectRange, Speed, SpeedVariability],
    OryxTaunt => [],
    PlaceMap => [Cooldown, Map, MaxPlace],
    PlaySound => [SoundId],
    Protect => [AcquireRange, ProtecteeId, ProtectionRange, Radius, Range, ReprotectRange, Speed],
    ReleaseSpawnAtDeath => [ChildId, Max, Min, Range],
    RemainVisible => [],
    RemoveOnEmpty => [],
    RestoresBreath => [Radius, Speed],
    Say => [Broadcast, BubbleTime, Cooldown, Prob, Text],
    SetAltTexture => [AltTextureId],
    SetGlobalTag => [SendNotification, TagName],
    SetTag => [SetGlobalTag, TagName],
    Shadow => [Range, ReshadowRange, Speed],
    Shoot => [AcquireRange, Angle, ArcDegrees, BlastEffect, ChildId, Cooldown, CooldownJitter, DefaultAngle, NoCooldownJitter, NumShots, Offset, Predictive, ProjectileId, Range, Type],
    ShowEffect => [Color, FlashPeriod, FlashRepeats, Type],
    StayAbove => [MinAltitude, Speed],
    StayBack => [Distance, Speed],
    StayCloseToSpawn => [Cooldown, Range, Speed],
    StayInTerrain => [Speed, Terrain, MinAltitude, MaxAltitude],
    Swirl => [Acceleration, Distance, Speed, TurnRate],
    Taunt => [Broadcast, ListId],
    TrapTrigger => [Color, Cooldown, CooldownJitter, MaxDamage, MinDamage, PosJitter, Radius, RadiusVariability, SoundId],
    VictoryAtDeath => [],
    Walk => [GroundTypes, Speed],
    Wander => [AvoidWater, Duration, Speed],
    Zap => [Color, ConditionDuration, ConditionEffect, Cooldown, Damage, PoisonDuration, PoisonTotalDamage, Range],
}
