Source: https://sites.google.com/decagames.com/xml-documentation/behaviors-transitions-list/stayinterrain

# StayInTerrain

**Effect:**

Monster will stay within a certain terrain (only in Realms)

**Attributes:**

- **terrain** (string, default empty): terrain to stay in
- **minAltitude** (int, default terrain params): altitude the monster will try to stay above
- **maxAltitude** (int, default terrain params): altitude the monster will try to stay below
- **speed** (float, default **0.5**): speed at which the monster moves

**Examples:**

Hermit God

```
<Behavior speed=".8" terrain="Dark Water">StayInTerrain</Behavior>
```

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
