Source: https://sites.google.com/decagames.com/xml-documentation/behaviors-transitions-list/wander

# Wander

**Effect:**

Move in a random direction

**Attributes:**

- **speed** (float, default **0.4**): speed at which the monster moves
- **avoidWater** (boolean, default **false**): whether the creature avoids water (currently only Water, Shallow Water and Dark Water count)

**Examples:**

Cube God

```
<Behavior avoidWater="true">Wander</Behavior>
```

Crystal Prisoner Clone

```
<Behavior speed="0.85">Wander</Behavior>
```

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
