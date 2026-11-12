Source: https://sites.google.com/decagames.com/xml-documentation/behaviors-transitions-list/zap

# Zap

**Effect:**

Zaps enemies

**Attributes:**

- **radius** (float, default **5.0**): maximum range
- **damage** (int, default **10**): amount of damage dealt
- **cooldown** (float, default **1.0**): time between zaps
- **color** (uint, default **0xFFFFFF**): color of zap
- **conditionEffect** (string, default **Nothing**): name of condition effect
- **conditionDuration** (float, default **0.0**): duration of effect
- **poisonTotalDamage** (int, default **0**): poison damage
- **poisonDuration** (float, default **0.0**): poison duration

**Examples:**

Orange Drake

```
<Behavior damage="0" cooldown="2.0" color="0xEB8931" conditionEffect="Stasis" conditionDuration="5">Zap</Behavior>
```

Yellow Drake

```
<Behavior damage="0" cooldown="0.5" color="0xEEEE73" conditionEffect="Bleeding" conditionDuration="10" poisonDuration="10" poisonTotalDamage="300">Zap</Behavior>
```

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
