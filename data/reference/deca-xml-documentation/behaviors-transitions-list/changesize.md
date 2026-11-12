Source: https://sites.google.com/decagames.com/xml-documentation/behaviors-transitions-list/changesize

# Change Size

**Effect:**

Causes a monster to change sprite size.

**Attributes:**

- **rate** (float, default **0.2**): rate of size change in percent per 0.2 seconds (for example 0.2 would be 1 percent a second)
- **minSize** (float, default **60.0**): size in percentage to stop shrinking at
- **maxSize** (float, default **200.0**): size in percentage to stop growing at

**Examples:**

Oryx the Mad God 1 (rage, starting Size is 100\)

```
<Behavior rate="10" maxSize="160">ChangeSize</Behavior>
```

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
