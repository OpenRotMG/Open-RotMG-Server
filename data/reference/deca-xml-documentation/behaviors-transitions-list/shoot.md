Source: https://sites.google.com/decagames.com/xml-documentation/behaviors-transitions-list/shoot

# Shoot

**Effect:**

Shoot projectile(s)

**Attributes:**

- **type** (string, default **targeted**):
  - **targeted**: monster will shoot at a player when nearby
  - **auto**: monster will shoot at a specified angle
  - **forward**: monster will shoot forward (monsters can have a facing based on certain behaviors)
- **range** (float, default **8.0**): distance from a player at which the behavior will be activated (targeted only)
- **numShots** (int, default **1**): number of projectiles in each shot
- **cooldown** (float, default **1.0**): time between shots in seconds
- **cooldownJitter** (boolean, default **false**): whether to vary the cooldown by a random amount (50% to 150% of cooldown)
- **projectileId** (int, default **0**): \<Projectile id\="x"\> tag to use, describes the projectile properties
- **childId** (string, default empty): create monster of this type instead of a projectile
- **angle** (float, default **9**): degrees between projectiles when numShots \> 1
- **predictive** (float, default **0.0**,between 0.0 and 1.0): the amount to which the monster attempts to predict where the player is going and shoot at its destination
- **defaultAngle**:
  - **targeted** (float, default **0**): added to the target angle (30 aims 30 degrees right of the target, -20 aims 20 degrees left of the target)
  - **auto** (float, default **90**): shot angle in degrees (0 or 360 \= east, 90 \= south, 180 \= west, 270 \= north)
- **arcDegrees** (float, default **0**): for monsters with facing, the possible firing arc centered on the facing angle, meaning it will only fire if a player is found within it
  - *Note: won't do anything if forward or auto is used for the shoot type*
- **blastEffect** (boolean, default **false**): whether to show [ConeBlast](http://i.imgur.com/OnUnnQy.gif) on shot
- **offset** (float, default **0.0**): specifying the time after this behavior becomes active for the first time
  - *Note: it is recommended that between offsets there are no intervals higher than 0.2 since minimum server intervals could otherwise result in stacked shots*
  - *Note: values not divisible by 0.2 will also lead to inconsistent intervals between shots as they are only processed every 0.2 seconds*
  - *Note: currently stuns and the enemy-off-screen bug can cause offsets to be ignored, can be worked around by using individual stages instead*

**Examples:**

Ent God

```
<Behavior projectileId="0" numShots="5" range="12" cooldown="1.25" angle="10" predictive="1">Shoot</Behavior>
```

Hobbit Mage (first ring)

```
<Behavior numShots="15" projectileId="0" cooldown="1.2" type="auto" angle="24" defaultAngle="0">Shoot</Behavior>
```

Eye of the Dragon (Dragon Head)

```
<Behavior type="forward" numShots="5" angle="10" projectileId="0" cooldown="0.4" range="0">Shoot</Behavior>
```

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
