Source: https://sites.google.com/decagames.com/xml-documentation/behaviors-transitions-list/objecttoss

# ObjectToss

**Effect:**

Throw an object

**Attributes:**

- **type** (string, default **targeted**):

  - **targeted**: monster will throw at a player when nearby
  - **auto**: monster will throw at a specified angle
  - **childId** (string, default empty): name of object to throw
  - **minRange** (float, default **0.0**): won’t target or throw less than this range
  - **range** (float, default **8.0**):
    - **targeted**: distance from a player at which the behavior will be activated
    - **auto**: how far the object will be thrown
  - **rangeJitter** (float, default **0.0**): vary range in “auto” mode (for example range of 5.0 and and rangeJitter of 2.0 means the shots will be uniformly distributed between 3.0 and 7.0)
  - **cooldown** (float, default **1.0**): seconds between throws
  - **cooldownJitter** (boolean, default **false**): whether to vary the cooldown by a random amount (50% to 150% of cooldown)
  - **defaultAngle** (float, default **90**):
    - **targeted**: no effect
    - **auto**: throw angle in degrees (0 or 360 \= east, 90 \= south, 180 \= west, 270 \= north)
  - **centerObject** (boolean, default **false**): whether objects should land at exactly the center of a square (x.5, y.5)

**Examples:**

Thessal the Mermaid Goddess (2nd phase)

```
<Behavior type="targeted" childId="Coral Bomb Big" rangeJitter="2" range="18" cooldown="1.6">ObjectToss</Behavior>
```

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
