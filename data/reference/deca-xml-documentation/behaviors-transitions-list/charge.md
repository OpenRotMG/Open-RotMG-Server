Source: https://sites.google.com/decagames.com/xml-documentation/behaviors-transitions-list/charge

# Charge

**Effect:**

Move straight toward a nearby player's position, then stop.

**Attributes:**

- **cooldown** (float, default **3.0**): time between moves in seconds
- **speed** (float, default **3.0**): the speed at which the monster moves
- **range** (float, default **20.0**): distance at which players can activate this behavior

**Examples:**

Ice Sphere

```
<Behavior cooldown="5" speed="2" range="20">Charge</Behavior>
```

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
