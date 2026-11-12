Source: https://sites.google.com/decagames.com/xml-documentation/behaviors-transitions-list/makeminions

# MakeMinions

**Effect:**

Create monsters

**Attributes:**

- **cooldown** (float, default **30.0** \| int, default **1**): time in seconds between creating new monster and (seperated with "\|") amount of monsters to be created per cooldown
- **cooldownJitter** (boolean, default **false**): whether to vary the cooldown by a random amount (50% to 150% of cooldown)
- **childIds** (string, default empty): comma-seperated list of monster types to create
- **groupId** (string, default empty): name of group to create monsters from (overrides childIds)
- **maxChildren** (int, default **0**): maximum number of minions to have alive at once
- **minOffset** (float, default **0.0**): minimum offset in a random direction in tiles
- **maxOffset** (float, default **0.0**): maximum offset in a random direction in tiles
- **startWithMinions** (boolean, default **true**): whether minions are created when the monster is
  - *Note: when in any state doesn't have to be in the root, the monsters will spawn immediately anyway if set to true*
  - *Note: monsters will automatically stop giving EXP after the second wave (meaning if the amount spawned is higher than maxChildren \* 2\)*

**Examples:**

Titanum

```
<Behavior cooldown="15|1" childIds="shtrs Stone Mage" maxChildren="5" startWithMinions="false">MakeMinions</Behavior> <Behavior cooldown="5|1" childIds="shtrs Stone Knight" maxChildren="5" startWithMinions="false">MakeMinions</Behavior>
```

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
