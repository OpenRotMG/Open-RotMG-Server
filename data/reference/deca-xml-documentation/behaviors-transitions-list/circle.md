Source: https://sites.google.com/decagames.com/xml-documentation/behaviors-transitions-list/circle

# Circle

**Effect:**

Move in a circle around the nearest player

**Attributes:**

- **acquireRange** (float, default **15.0**): distance at which players can activate this behavior
- **distance** (float, default **8.0**): distance to stay from players
- **speed** (float, default **1.0**): speed at which the monster moves

**Examples:**

Greater Nature Sprite

```
<Behavior acquireRange="7" distance="4" speed="1.5">Circle</Behavior>
```

Ice Adept

```
<Behavior acquireRange="20" distance="2" speed="1">Circle</Behavior>
```

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
