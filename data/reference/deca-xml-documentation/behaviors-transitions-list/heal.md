Source: https://sites.google.com/decagames.com/xml-documentation/behaviors-transitions-list/heal

# Heal

**Effect:**

Heal other monster of given types

**Attributes:**

- **targetIds** (string, default empty): comma-separated list of object ids to heal
- **targetGroups** (string, default empty): comma-separated list of group names to heal
- **cooldown** (float, default **10.0**): time in seconds between heals
- **range** (float, default **10.0**): max distance to heal at
- **maxTargets** (int, default **-1** meaning infinite): number of targets that can be healed
- **maxHeal** (int, default **-1** meaning infinite): the amount of healing that can be done
- **hideVisual** (boolean, default **false**): whether to hide all visuals

**Examples:**

Rock Construct

```
<Behavior targetGroups="Papers" cooldown="1">Heal</Behavior>
```

Worshipping Priest and Worshipping Priestess (Tomb Beam Priests)

```
<Behavior targetIds="Active Sarcophagus" cooldown="0.4" range="10" maxTargets="1" maxHeal="200">Heal</Behavior>
```

Masked Party God

```
<Behavior targetIds="Masked Party God" cooldown="0.2" range="1" maxHeal="3000" hideVisual="true">Heal</Behavior>
```

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
