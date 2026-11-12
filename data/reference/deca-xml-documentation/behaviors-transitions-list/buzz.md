Source: https://sites.google.com/decagames.com/xml-documentation/behaviors-transitions-list/buzz

# Buzz

**Effect:**

Run toward and through a nearby player (gives facing).

**Attributes:**

- **speed** (float, default **15.0**): speed at which the monster moves
- **acceleration** (float, default **5.0**): rate at which the speed increases
- **turnRate** (float, default **Pi**): rate of turn (negative values send object in opposite direction)
- **acquireRange** (float, default **20.0**): distance at which players can activate this behavior
- **cooldown** (float, default **2.0**): seconds between buzzes

**Examples:**

Pentaract Eye

```
<Behavior bucket="movement">Buzz</Behavior>
```

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
