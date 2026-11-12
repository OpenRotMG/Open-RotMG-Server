Source: https://sites.google.com/decagames.com/xml-documentation/behaviors-transitions-list/follow

# Follow

**Effect:**

Move toward a nearby player

**Attributes:**

- **acquireRange** (float, default **10.0**): distance at which players can activate this behavior
- **range** (float, default **3.0**): distance determining how close to get to a player
- **speed** (float, default **1.0**): the speed at which the monster moves
- **duration** (float, default **30.0**): how long to follow a player in seconds
- **cooldown** (float, default **5.0**): how long to wait between follows in seconds
- **predictive** (boolean, default **false**): whether to try to predict the player position

**Examples:**

Djinn

```
<Behavior range="0.5" speed="0.7">Follow</Behavior>
```

**Sea Mare**

```
<Behavior acquireRange="10" duration="1.6" cooldown=".6" speed="1.4" range="2.5" predictive="true">Follow</Behavior>
```

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
