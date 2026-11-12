Source: https://sites.google.com/decagames.com/xml-documentation/behaviors-transitions-list/releasespawnatdeath

# ReleaseSpawnAtDeath

**Effect:**

Create monsters at this monster's death

**Attributes:**

- **min** (int, default **2**): minimum number of new monsters
- **max** (int, default **7**): maximum number of new monsters
- **childId** (string, default empty): name of type of monsters to spawn

**Examples:**

Large Ghost (Ghost King minion)

```
<Behavior min="2" max="3" childId="Imp">ReleaseSpawnAtDeath</Behavior>
```

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
