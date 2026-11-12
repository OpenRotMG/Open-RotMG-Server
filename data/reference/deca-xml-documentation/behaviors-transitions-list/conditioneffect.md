Source: https://sites.google.com/decagames.com/xml-documentation/behaviors-transitions-list/conditioneffect

# ConditionEffect

**Effect:**

Set a condition effect on the monster

**Attributes:**

- **effect** (string, default empty): name of the effect (see the list at end of this document)
- **duration** (float, default **0.0**): how long the effect will last
  - *Note: 0.0 means permanent while behavior is active*
  - *Note 2: ConditionEffects will be removed if you transition out of the state with this behavior*

**Examples:**

Djinn (between attacks)

```
<Behavior effect="Invulnerable">ConditionEffect</Behavior>
```

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
