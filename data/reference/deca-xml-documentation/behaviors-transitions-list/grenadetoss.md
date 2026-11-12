Source: https://sites.google.com/decagames.com/xml-documentation/behaviors-transitions-list/grenadetoss

# GrenadeToss

**Effect:**

Throw an AoE (Area of Effect) grenade

**Attributes:**

- **type** (string, default **targeted**):

  - **targeted**: monster will throw at a player when nearby
  - **auto**: monster will throw at a specified angle
  - **minRange** (float, default **0.0**): won’t target or throw less than this range
  - **range** (float, default **8.0**):
    - **targeted**: distance from a player at which the behavior will be activated
    - **auto**: how far the grenade will be thrown
  - **cooldown** (float, default **1.0**): seconds between throws
  - **cooldownJitter** (boolean, default **false**): whether to vary the cooldown by a random amount (50% to 150% of cooldown)
  - **radius** (float, default **1.0**): radius of damage
  - **damage** (int, default **10**): amount of damage done
  - **defaultAngle** (float, default **90**):
    - **targeted**: no effect
    - **auto**: throw angle in degrees (0 or 360 \= east, 90 \= south, 180 \= west, 270 \= north)
  - **effect** (string, default empty): the name of the condition effect to apply if hit
    - *Note: currently there can be a bug which causes effects to last forever (like Confused)*
  - **duration** (int, default **10**): duration of the condition effect
  - **textureSourceId** (string, default empty): object id of which the texture is used for the throw effect (will overwrite particles)

**Examples:**

Medusa

```
<Behavior range="8" cooldown="3" radius="4" damage="150">GrenadeToss</Behavior>
```

Beach Ball

```
<Behavior type="targeted" range="10" cooldown="2" radius="0" damage="0" minRange="0" textureSourceId="Beach Ball Bullet">GrenadeToss</Behavior>
```

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
