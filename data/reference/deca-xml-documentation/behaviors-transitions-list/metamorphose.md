Source: https://sites.google.com/decagames.com/xml-documentation/behaviors-transitions-list/metamorphose

# Metamorphose

**Effect:**

Replace monster with a different monster

**Attributes:**

- **time** (float, default **60.0**): seconds before metamorphosis
- **childId** (string, default empty): name of monster to replace this monster with
- **dropLoot** (boolean, default **false**): whether to drop loot
- **giveXP** (boolean, default **false**): whether to give xp to nearby player

**Examples:**

Geb Statue

```
<Behavior time="2.0" childId="Tomb Attacker">Metamorphose</Behavior>
```

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
