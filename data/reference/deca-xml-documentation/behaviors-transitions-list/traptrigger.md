Source: https://sites.google.com/decagames.com/xml-documentation/behaviors-transitions-list/traptrigger

# TrapTrigger

**Effect:**

Causes an AoE (Area of Effect) explosion that damage enemies

**Attributes:**

- **radius** (float, default **1.0**): radius of explosion
- **radiusVariability** (float, default **0.0**): % variability for radius
- (for example 0.5 means \+/- 50% of radius)
- **minDamage** (int, default **10**): minimum amount of damage
- **maxDamage** (int, default **10**): maximum amount of damage
- **posJitter** (float, default **0.0**): randomize position
- **conditionEffect** (string, default **Nothing**): name of condition effect
- **conditionDuration** (float, default **5.0**): duration of effect
- **cooldown** (float, default **3.0**): time between explosions
- **cooldownJitter** (float, default **0.0**): randomize cooldown
- (for example 3.0 and jitter of 1.0 would result in cooldown between 2.0 and 4.0)
- **color** (uint, default **0xAA00FF**): comma-seperated list of explosion colors
- **soundId** (int, default **-1**): \<Sound id\="x"\> tag to use, specifies the sound

**Examples:**

Lil Firecracker and Big Firecracker

```
<Behavior radius="3" minDamage="25" maxDamage="80" radiusVariability="0.4" posJitter="1.0" color="0xf41b13,0x1a66f2,0xf2e6e6" cooldown="0.4" cooldownJitter="0.39" soundId="0">TrapTrigger</Behavior>
```

Spirit Prism Bomb

```
<Behavior radius="2.4" minDamage="40" maxDamage="90" radiusVariability="0" posJitter="1.6" color="0xff5132" cooldown="0.15" cooldownJitter="0" soundId="0">TrapTrigger</Behavior>
```

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
