Source: https://sites.google.com/decagames.com/xml-documentation/behaviors-transitions-list/metamorphoseatdeath

# MetamorphoseAtDeath

**Effect:**

Replace a monster with a different monster on death

**Attributes:**

- **prob** (float, default **1.0**): probability of the metamorphosis taking place
- **posJitter** (float, default **0.1**): distance in a random direction the new object will be created
- **childId** (string, default empty): name of monster to replace this dead monster with
  - *Note: will also be triggered by Decay and Metamorphose behaviors, be careful especially with the latter, if both Metamorphose and MetamorphoseAtDeath behaviors are active at the same time, both will spawn the childId*

**Examples:**

Limon

```
<Behavior prob="1" childId="Glowing Realm Portal">MetamorphoseAtDeath</Behavior>
```

```
<Behavior prob="1" childId="Limon SafeGround">MetamorphoseAtDeath</Behavior>
```

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
