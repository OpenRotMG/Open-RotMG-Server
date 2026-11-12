Source: https://sites.google.com/decagames.com/xml-documentation/behaviors-transitions-list/keepdistance

# KeepDistance

**Effect:**

Attempt to stay at exactly a certain distance from players

**Attributes:**

- **acquireRange** (float, default **15.0**): distance at which players can activate this behavior
- **distance** (float, default **10.0**): distance to stay from players
- **speed** (float, default **1.0**): speed at which the monster moves

**Examples:**

Sea Horse

```
<Behavior acquireRange="4" distance="3.5" speed=".5">KeepDistance</Behavior>
```

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
