Source: https://sites.google.com/decagames.com/xml-documentation/behaviors-transitions-list/maintainprotectors

# MaintainProtectors

**Effect:**

Maintain multiple enemies and die if all "protectors" are dead

**Attributes:**

- **cooldown** (int, default **30**): time between respawns (is randomized 50% to 150%)
- **childIds** (string, default empty): comma-seperated list of monster types to create
- **childCorpseId** (string, default empty): corpse id (protectors turn into when dead)
- **groupId** (string, default empty): name of group to create monsters from (overrides childIds)
- **numChildren** (int, default **2**): amount of protectors (automatically arranged in a surrounding circle)
- **distance** (int, default **0**): distance of objects (circle radius)

**Examples:**

Pentaract

```
<Behavior childIds="Pentaract Tower" childCorpseId="Pentaract Tower Corpse" cooldown="15" numChildren="5" distance="15">MaintainProtectors</Behavior>
```

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
