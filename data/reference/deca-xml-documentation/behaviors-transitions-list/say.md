Source: https://sites.google.com/decagames.com/xml-documentation/behaviors-transitions-list/say

# Say

**Effect:**

Send single chat message

**Attributes:**

- **text** (string, default empty): semi-colon separated list of possible chat messages (e.g. “foo bar;baz bap” will say “foo bar” half the time and “baz bap” the other)
- **broadcast** (boolean, default **false**): whether to yell or just chat
- **prob** (float, default **1.0**): probability of a chat message being sent
- **bubbleTime** (int, default **10**): number of seconds to show the bubble for

**Examples:**

Steel Construct

```
<Behavior prob=".05" text="Silly squishy. We heal our brothers in a circle.">Say</Behavior>
```

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
