Source: https://sites.google.com/decagames.com/xml-documentation/behaviors-transitions-list/walk

# Walk

**Effect:**

Move to a nearby square of a certain terrain type

**Attributes:**

- **speed** (float, default **0.4**): speed at which the monster moves
- **groundTypes** (string, default empty): comma-separated list of ground types to move to

**Examples:**

Sheep

```
<Behavior groundTypes="Light Grass,Yellow Grass,Grass,Dark Grass,Bright Grass">Walk</Behavior>
```

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
