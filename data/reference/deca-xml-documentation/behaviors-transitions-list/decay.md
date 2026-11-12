Source: https://sites.google.com/decagames.com/xml-documentation/behaviors-transitions-list/decay

# Decay

**Effect:**

Kill a monster after a certain amount of time

**Attributes:**

- **time** (float, default **60.0**): seconds before killing monster
- **particleEffect** (string, default empty): name of particle effect to trigger (currently only “Nova” supported)
- **color** (uint, default **0xFFFFFF**): color of Nova effect
- **dropLoot** (boolean, default **false**): whether to drop loot
- **giveXP** (boolean, default **false**): whether to give xp to nearby players
- **resignPet** (boolean, default **false**): resigns pet for the owner (used by certain temporary allies)
  - *Note: explode and bullet are deprecated, use Explode behavior instead*
- **explode** (boolean, default **false**): whether to explode on decay
- **bullet** (int, default **0**): bullet amount for explosion

**Examples:**

Orange Drake

```
<Behavior time="30" particleEffect="Nova" color="0xEB8931">Decay</Behavior>
```

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
