Source: https://sites.google.com/decagames.com/xml-documentation/behaviors-transitions-list/clearconditioneffectaura

# ClearConditionEffectAura

**Effect:**

Set a condition effect clearing aura on the monster

**Attributes:**

- **effect** (string, default empty): comma-separated list of effects (see the list at end of this document)
- **ignoreInvisible** (boolean, default **true**): whether to ignore invisible objects
- **range** (float, default **0.0**): range of the clear
- **cooldown** (float, default **1.0**): time between clears
  - Note: ClearConditionEffectAura will be removed if you transition out of the state with this behavior

**Examples:**

DeHexxer (in Mad Lab liquid)

```
<Behavior effect="Stunned,Speedy,Hexed" cooldown=".2" range="2" ignoreInvisible="false">ClearConditionEffectAura</Behavior>
```

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
