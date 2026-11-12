Source: https://sites.google.com/decagames.com/xml-documentation/behaviors-transitions-list/orbit

# Orbit

**Effect:**

Circle around a given monster type (gives facing)

**Attributes:**

- **protecteeId** (string, default empty): name of monster type to orbit
- **clockwise** (boolean, default **false**): determines direction of orbit
- **acquireRange** (float, default **15.0**): distance within which a monster will look for a protectee
- **speed** (float, default **0.8**): speed at which the monster moves
- **radius** (float, default **5.0**): distance at which the monster orbits
- **speedVariability** (float, default **0.2**): % variability for speed
- (for example 0.2 means \+/- 20% of speed when Orbit is created)
- **radiusVariability** (float, default **0.5**): % variability for radius
- (for example 0.5 means \+/- 50% of radius when Orbit is created)
- **radiusWobbleDistance** (float, default **0.0**): amplitude of sine wave added to radius
- **radiusWobblePeriod** (float, default **15.0**): period of sine wave added to radius
  - *Note: if the monster gets stuck, it will change direction and circle back in the opposite direction (paralyze, wall, other object)*

**Examples:**

Nut (waiting and first phase)

```
<Behavior protecteeId="Tomb Boss Anchor" radius="4.8" speed="0.6">Orbit</Behavior>
```

Nexus Chinese Dragon

```
<Behavior protecteeId="Nexus Anchor" speedVariability="0.1" speed=".4" radius="9" radiusVariability="0.1" radiusWobbleDistance="1.5" radiusWobblePeriod="10">Orbit</Behavior>
```

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
