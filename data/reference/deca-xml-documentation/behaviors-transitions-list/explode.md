Source: https://sites.google.com/decagames.com/xml-documentation/behaviors-transitions-list/explode

# Explode

**Effect:**

Remove monster and send a spray of projectiles in all directions

**Attributes:**

- **initialDelay** (float, default **0.0**): seconds before exploding
- **range** (float, default **5.0**): distance for player to activate. 0.0 range means it does not need activation
- **numShots** (int, default **12**): number of projectiles
- **arcDegrees** (int, default **360**): arc of the shot
- **projectileId** (int, default **0**): \<Projectile\> tag that describes the projectile properties
- **dropLoot** (boolean, default **false**): whether to drop loot
- **giveXP** (boolean, default **false**): whether to give xp to nearby player

**Examples:**

Djinn

```
<Behavior range="0.0" projectileId="0" dropLoot="true" giveXP="true">Explode</Behavior>
```

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
