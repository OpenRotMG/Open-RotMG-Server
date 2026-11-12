Source: https://sites.google.com/decagames.com/xml-documentation/behaviors-transitions-list/conditioneffectaura

# ConditionEffectAura

**Effect:**

Set a condition effect aura on the monster

**Attributes:**

- **effect** (string, default empty): comma-separated list of effects (see the list at end of this document)
- **duration** (float, default **-1.0**): how long the effect will last
  - Note: less than 0.0 means permanent while behavior is active
- **range** (float, default **0.0**): range of the aura
- **cooldown** (float, default **1.0**): time between active auras
- Note: ConditionEffectAura will be removed if you transition out of the state with this behavior

**Examples:**

Hexxer (in Mad Lab liquid)

```
<Behavior effect="Stunned,Speedy,Hexed" cooldown=".2" range="2">ConditionEffectAura</Behavior>
```

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
