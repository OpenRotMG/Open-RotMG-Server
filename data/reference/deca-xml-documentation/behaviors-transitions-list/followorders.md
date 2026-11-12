Source: https://sites.google.com/decagames.com/xml-documentation/behaviors-transitions-list/followorders

# FollowOrders

**Effect:**

Have the monster follow behaviors specified by another monster

**Attributes:**

- **listenIds** (string, default empty): comma-separated list of monster types to listen to
- **acquireRange** (float, default **15.0**): distance max at which a monster will look for an orderer
- **cooldown** (float, default **3.0**): seconds between new orders

**Examples:**

Horned Drake

```
<Behavior listenIds="Drake Baby">FollowOrders</Behavior>
```

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
