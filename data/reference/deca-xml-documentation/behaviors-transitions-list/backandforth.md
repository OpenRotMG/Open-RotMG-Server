Source: https://sites.google.com/decagames.com/xml-documentation/behaviors-transitions-list/backandforth

# BackAndForth

**Effect:**

Makes a monster move back and forth around its current location.

**Attributes:**

- **speed** (float, default 1.0): speed at which the monster moves
- **angle** (float, default 0.0): angle of the movement (0 means x axis)
- **turnRate** (float, default 0.0): change in angle over time
- **radius** (float, default 4.0): radius of the movement from the original position

**Examples:**

Actual Ghost King ("I am still so very alone...")

```
<Behavior radius="3" speed="0.5">BackAndForth</Behavior>
```

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
