Source: https://sites.google.com/decagames.com/xml-documentation/behaviors-transitions-list/protect

# Protect

**Effect:**

Keep close to a monster of the given type (gives facing)

**Attributes:**

- **protecteeId** (string, default empty): name of monster to protect
- **acquireRange** (float, default **15.0**): distance within which a monster will look for a protectee
- **protectionRange** (float, default **3.0**): distance beyond which monster will attempt to “reprotect” the protectee
- **reprotectRange** (float, default **protectionRange**): distance to get within protectee (should be \<\= protectionRange)
- **speed** (float, default **0.8**): speed at which the monster moves when reprotecting

**Examples:**

Horned Drake (ordered by Drake Baby at low health)

```
<Behavior protecteeId="Drake Baby" speed="0.8" acquireRange="12" protectionRange="2.5" reprotectRange="1.5">Protect</Behavior>
```

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
