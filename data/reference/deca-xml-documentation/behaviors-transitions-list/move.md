Source: https://sites.google.com/decagames.com/xml-documentation/behaviors-transitions-list/move

# Move

**Effect:**

Have a monster move in a straight line

**Attributes:**

- **speed** (float, default **1.0**): speed at which the monster moves
- **distance** (float, default **3.0**): how far the monster moves
- **angle** (float, default **0.0**): direction in degrees (0 or 360 \= east, 90 \= south, 180 \= west, 270 \= north)
- **cooldown** (float, default **10.0**): interval between moves

**Examples:**

The Forgotten King (after spawning guards)

```
<Behavior speed="0.7" distance="9" angle="90" cooldown="100000000000000">Move</Behavior>
```

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
