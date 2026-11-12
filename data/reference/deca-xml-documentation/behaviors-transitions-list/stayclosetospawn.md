Source: https://sites.google.com/decagames.com/xml-documentation/behaviors-transitions-list/stayclosetospawn

# StayCloseToSpawn

**Effect:**

Have a monster stay close to the point it spawned at

**Attributes:**

- **speed** (float, default **1.0**): speed at which the monster moves
- **cooldown** (float, default **3.0**): seconds between moving closer to spawn
- **range** (float, default **7.0**): max distance to get from spawn

**Examples:**

Geb (rage)

```
<Behavior range="7" speed="4" cooldown="10">StayCloseToSpawn</Behavior>
```

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
