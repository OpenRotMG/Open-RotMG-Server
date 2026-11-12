Source: https://sites.google.com/decagames.com/xml-documentation/xml-guide/condition-effects

# Condition Effects

### Here are two examples of applied Condition Effects:

**Djinn (between attacks)**

```
<Behavior effect="Invulnerable">ConditionEffect</Behavior>
```

This condition effect is used as a "Behavior" and will apply "Invulnerable" to the Djinn when triggered.

**Condition effect on Projectile:**

```
<Projectile id="4">
```

```
<ObjectId>Green Star</ObjectId>
```

```
<Speed>80</Speed>
```

```
<Damage>60</Damage>
```

```
<LifetimeMS>2000</LifetimeMS>
```

```
<ConditionEffect duration="9">Slowed</ConditionEffect>
```

```
</Projectile>
```

This condition effect is used in a projectile and will be applied to the target that is hit by the projectile.

## List of Condition Effects:

### Players only:

- **Quiet**: Mana is reduced to 0 and remains there for the duration of the effect (i.e. no special ability).
- **Weak**: Attack stat is effectively set to 0
- **Sick**: No hitpoint regeration
- **Blind**: Player’s screen goes dark
- **Hallucinating**: Textures are randomly swapped
- **Drunk**: Player’s screen gets blurry
- **Confused**: Player’s movement keys are remapped
- **Invisible**: Can not be targeted by monsters
- **Bleeding**: Slowly lose health (instead of regenerating)
- **Healing**: Hitpoint regeneration greatly increased
- **Speedy**: Movement speed increased by 50%
- **Berserk**: Attack speed increased by 50%
- **Damaging**: Damage increased by 50%
- **Paused**: Player can not interact with world
- **Pet Stasis:** Player's pet is unable to cast for the duration of the effect
- **Petrify:**

### Players and enemies:

- **Slowed**: Speed is effectively set to 0 for players or halved for monsters
- **Paralyzed**: Can not move
- **Stasis**: Can not move/interact with world
- **Stasis Immune**: Can not become stasis
- **Invincible**: Projectiles and other damage ignore character (go through)
- **Invulnerable**: Projectiles and other damage do not harm the creature (but will still hit)
- **Armored**: Defense is doubled
- **Armor Broken**: Defense is set to 0
- **Dazed**: Halves number of projectiles shot
- **Stunned**: Can not shoot

### [Next: Embed code \>](embed-code.md)

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
