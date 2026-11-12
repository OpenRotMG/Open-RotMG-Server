Source: https://sites.google.com/decagames.com/xml-documentation/xml-guide/embed-code

# Embed code

By defining Embeds, you can create type behaviors and assign a name to them.

This allows you to call them in various places in your code by simply using the name of an existing Embed instead of typing the same behavior again multiple times.

Let's have a look at the explanation below:

Let's create a new Embed attack called "Pulsing" using the "[Shoot](../behaviors-transitions-list/shoot.md)" behavior:

```
<DefineEmbed name="Pulsing">
```

```
<Behavior numShots="10" projectileId="0" cooldown="1" type="auto" defaultAngle="0" offset="0.0"       angle="36">Shoot</Behavior>
```

```
</DefineEmbed>
```

We can now use the same "Embed" in two different states, simply by calling the embed name that we defined when creating it:

```
<State id="Invul">
```

```
<Behavior altTextureId="0">SetAltTexture</Behavior>
```

```
<Behavior effect="Invulnerable">ConditionEffect</Behavior>
```

```
<Embed name="Pulsing"></Embed>
```

```
</State>
```

```
<State id="NotInvul">
```

```
<Behavior altTextureId="1">SetAltTexture</Behavior>
```

```
<Embed name="Pulsing"></Embed>
```

```
<Embed name="Healing"></Embed>
```

```
</State>
```

### [Next: Behaviors List \>](../behaviors-transitions-list.md)

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
