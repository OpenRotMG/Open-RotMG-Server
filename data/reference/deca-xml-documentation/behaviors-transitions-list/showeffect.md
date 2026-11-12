Source: https://sites.google.com/decagames.com/xml-documentation/behaviors-transitions-list/showeffect

# ShowEffect

**Effect:**

Trigger an effect

**Attributes:**

- **type** (string, default **Flash**): type of effect (Flash only currently)
- **color** (uint, default **0xFFFFFF**): color of the effect
- **flashPeriod** (float, default **0.75**): time in seconds of a single flash
- **flashRepeats** (float, default **1.0**): number of flashes

**Examples:**

Djinn (between attacks)

```
<Behavior type="Flash" color="00FF00" flashPeriod="0.1" flashRepeats="20">ShowEffect</Behavior>
```

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
