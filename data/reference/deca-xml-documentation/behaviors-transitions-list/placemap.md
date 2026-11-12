Source: https://sites.google.com/decagames.com/xml-documentation/behaviors-transitions-list/placemap

# PlaceMap

**Effect:**

Place a .jm file under the object

**Attributes:**

- **map** (string, default empty): location of map file (.jm)
- **maxPlace** (int, default **1**): how many times to do this action (-1 \= infinite)
- **cooldown** (float, default **5.0**): time between placing maps (inactive if maxPlace is 1\)

**Examples:**

Pentaract Tower

```
<Behavior map="encounter_maps/pentaract_map.jm">PlaceMap</Behavior>
```

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
