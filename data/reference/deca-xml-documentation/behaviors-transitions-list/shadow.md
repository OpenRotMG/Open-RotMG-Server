Source: https://sites.google.com/decagames.com/xml-documentation/behaviors-transitions-list/shadow

# Shadow

**Effect:**

Follows player (used by summoned allies)

**Attributes:**

- **range** (float, default **3.0**): maximum distance before attempting to come close again
- **speed** (float, default **3.0**): speed at which the monster moves, affected by range and how far away the monster is (speed \*\= dist / range\_)
- **reshadowRange** (float, default **0.5**): how close monster attempts to come once out of range
  - *Note: will teleport to player when more than 30 tiles away*

**Examples:**

All Drakes

```
<Behavior range="2.5" speed="1.0">Shadow</Behavior>
```

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
