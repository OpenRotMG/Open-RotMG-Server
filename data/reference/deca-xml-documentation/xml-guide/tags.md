Source: https://sites.google.com/decagames.com/xml-documentation/xml-guide/tags

# Tags

Tags can be used to tally action, communicate between enemy brains and set up basic "***IF***" statements.

Let's take an example:

```
 <State id="Stand">
```

```
        <State id="Sub1">
```

```
            <Behavior prob="1" text="Sub1">Say</Behavior>
```

```
            <Transition afterTime="5">Sub2</Transition>
```

```
        </State>
```

```
        <State id="Sub2">
```

```
            <Behavior prob="1" text="Sub2">Say</Behavior>
```

```
            <Behavior tagName="SubsDone">SetTag</Behavior>
```

```
        </State>
```

```
        <Transition hasTag="SubsDone">Wander</Transition>
```

```
    </State>
```

```
    <State id="Wander">
```

```
        <State id="Sub1">
```

```
            <Behavior prob="1" text="Sub1">Say</Behavior>
```

```
            <Transition afterTime="5">Sub2</Transition>
```

```
        </State>
```

```
        <State id="Sub2">
```

```
            <Behavior prob="1" text="Sub2">Say</Behavior>
```

```
            <Behavior tagName="SubsDone">ClearTag</Behavior>
```

```
        </State>
```

```
        <Behavior bucket="movement">Wander</Behavior>
```

```
        <Transition noTag="SubsDone">Stand</Transition>
```

```
    </State>
```

### [Next: Projectiles \>](projectiles.md)

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
