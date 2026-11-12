Source: https://sites.google.com/decagames.com/xml-documentation/behaviors-transitions-list/healzap

# HealZap

**Effect:**

Heal players

**Attributes:**

- **radius** (float, default **5.0**): maximum range
- **healing** (int, default **25**): amount of hp healed per zap
- **cooldown** (float, default **1.0**): time between zaps
- **color** (uint, default **0xFFFFFF**): color of zap

**Examples:**

White Fountain

```
<Behavior radius="5" healing="100" cooldown="1.0" color="0xFFFFFF">HealZap</Behavior>
```

White Drake

```
<Behavior healing="25" cooldown=".5" color="0xFFFFFF">HealZap</Behavior>
```

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
