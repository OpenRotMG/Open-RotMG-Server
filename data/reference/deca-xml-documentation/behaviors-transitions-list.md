Source: https://sites.google.com/decagames.com/xml-documentation/behaviors-transitions-list

# Behaviors \& Transitions list

Behaviors are the basic unit of specifying how enemies act in the world. They have some [rules](xml-guide.md) that determine how they are executed.

In this section, you will find all Behaviors that can be used to mod monster in XML.

### Behaviors:

- [BackAndForth](behaviors-transitions-list/backandforth.md)
- [Buzz](behaviors-transitions-list/buzz.md)
- [ChangeGround](behaviors-transitions-list/changeground.md)
- [ChangeSize](behaviors-transitions-list/changesize.md)
- [Charge](behaviors-transitions-list/charge.md)
- [ConditionEffectAura](behaviors-transitions-list/conditioneffectaura.md)
- [Circle](behaviors-transitions-list/circle.md)
- [ClearConditionEffect](behaviors-transitions-list/clearconditioneffectaura.md)
- [ConditionEffect](behaviors-transitions-list/conditioneffect.md)
- [CopyDamageRecord](behaviors-transitions-list/copydamagerecord.md)
- [Decay](behaviors-transitions-list/decay.md)
- [DeleteObject](behaviors-transitions-list/deleteobject.md)
- [Explode](behaviors-transitions-list/explode.md)
- [Follow](behaviors-transitions-list/follow.md)
- [FollowOrders](behaviors-transitions-list/followorders.md)
- [GrenadeToss](behaviors-transitions-list/grenadetoss.md)
- [Heal](behaviors-transitions-list/heal.md)
- [HealZap](behaviors-transitions-list/healzap.md)
- [KeepDistance](behaviors-transitions-list/keepdistance.md)
- [MakeMinions](behaviors-transitions-list/makeminions.md)
- [MaintanProtectors](behaviors-transitions-list/maintainprotectors.md)
- [Metamorphose](behaviors-transitions-list/metamorphose.md)
- [MetamorphoseAtDeath](behaviors-transitions-list/metamorphoseatdeath.md)
- [Move](behaviors-transitions-list/move.md)
- [ObjectToss](behaviors-transitions-list/objecttoss.md)
- [Orbit](behaviors-transitions-list/orbit.md)
- [PlaceMap](behaviors-transitions-list/placemap.md)
- [PlaySound](behaviors-transitions-list/playsound.md)
- [Protect](behaviors-transitions-list/protect.md)
- [ReleaseSpawnAtDeath](behaviors-transitions-list/releasespawnatdeath.md)
- [Say](behaviors-transitions-list/say.md)
- [SetAltTexture](behaviors-transitions-list/setalttexture.md)
- [Shadow](behaviors-transitions-list/shadow.md)
- [Shoot](behaviors-transitions-list/shoot.md)
- [ShowEffect](behaviors-transitions-list/showeffect.md)
- [StayAbove](behaviors-transitions-list/stayabove.md)
- [StayCloseToSpawn](behaviors-transitions-list/stayclosetospawn.md)
- [Swirl](behaviors-transitions-list/swirl.md)
- [TrapTrigger](behaviors-transitions-list/traptrigger.md)
- [Walk](behaviors-transitions-list/walk.md)
- [Wander](behaviors-transitions-list/wander.md)
- [Zap](behaviors-transitions-list/zap.md)

### Transitions:

- `afterTime`: Transition immediately after the specified number of seconds.
- `hitpointsLessThan`: Transition when the hitpoints of the creature fall below the specified proportion (i.e. 0.5 is 50%).
- `playerWithin`: A player is within the specified number of squares. (distance\|number of players)
- `noPlayerWithin`: There is no player within the specified number of squares.
- `noneExist`: There are no objects of the specified type in this dungeon
- `exist`: There are objects of the specified type in this dungeon
- `random`: Transition with specified probability on each tick (e.g. random\=”.05” has a 5% probability to take this transition every .2 seconds)
- `playerSays`: Transition when a player within 30 squares says the given phrase (ignoring case and whitespace)
- `hasTag`: Transition if this tag is set
- `noTag`: Transition if this tag is not set
- `onTile`: Transition if on a certain tile (\<Transition onTile\="tile1, tile2, tile3"\>StatetoTransition\</Transition\>

[**Leave feedback**](feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
