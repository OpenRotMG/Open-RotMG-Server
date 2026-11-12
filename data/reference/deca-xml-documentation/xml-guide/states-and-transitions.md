Source: https://sites.google.com/decagames.com/xml-documentation/xml-guide/states-and-transitions

# States and Transitions

Buckets are one form of control flow for Behaviors, the next are ***states and transitions***.

You can see a list of all available Transitions [here](../behaviors-transitions-list.md).

Let's take another example:

### The Test Chicken Example

```
<State id="Attacking">
```

```
  <State id="Bullet1">
```

```
    <Behavior numShots="3" projectileId="0"cooldown="0.6">Shoot</Behavior>
```

```
    <Transition afterTime="1.2">Wait1</Transition>
```

```
  </State>
```

```
  <State id="Wait1">
```

```
    <Transition afterTime="1">Bullet2</Transition>
```

```
  </State>
```

```
  <State id="Bullet2">
```

```
    <Behavior projectileId="1" cooldown="10.0">Shoot</Behavior>
```

```
    <Transition afterTime="1.0">Bullet1</Transition>
```

```
  </State>
```

```
    <Behavior bucket="movement">Follow</Behavior>
```

```
    <Behavior bucket="movement">Wander</Behavior>
```

```
    <Transition hitpointsLessThan="0.5">Flash</Transition>
```

```
</State>
```

```

```

```
<State id="Flash">
```

```
  <Behavior type="Flash" color="FF0000">ShowEffect</Behavior>
```

```
  <Transition afterTime="1.0">Explode</Transition>
```

```
</State>
```

```

```

```
<State id="Explode">
```

```
  <Behavior range="0.0" projectileId="0">Explode</Behavior>
```

```
</State>
```

```
<Behavior>Taunt</Behavior>
```

### States work by the following rules:

- An object is always in exactly one high-level state and starts in the first state listed (e.g. “***Attacking***”). If that state has sub-states, the same rules apply (only in one sub-state, starting with first, e.g. “***Bullet1***”). There can be an arbitrary number of levels of states.
- All behaviors in the current states are executed. For example, in the starting state “***Attacking/Bullet1***”, it would execute the following behaviors:

```
<Behavior numShots="3" projectileId="0"
```

```
cooldown="0.6">Shoot</Behavior>
```

```
<Behavior bucket="movement">Follow</Behavior>
```

```
<Behavior bucket="movement">Wander</Behavior>
```

```
<Behavior>Taunt</Behavior>
```

Shoot comes from Bullet1 and Follow and Wander come from Attacking and since Taunt is outside of all states, it is always executed.

- The state changes when the conditions of a Transition tag are met and then the object will transition to the tag named. Transitions can only cause changes to sibling states. They can not cause transitions to different top-level (“uncle”) states. So after 1.2 seconds, the Test Chicken will transition from “***Attacking/Bullet1***” to “***Attacking/Wait1***”.

### The Test Chicken XML explained step-by-step

*1 \- The Test Chicken will shoot 3 times a barrage of 3 bullets.*

```
<State id="Attacking">
```

```
  <State id="Bullet1">
```

```
    <Behavior numShots="3" projectileId="0"cooldown="0.6">Shoot</Behavior>
```

```
    <Transition afterTime="1.2">Wait1</Transition>
```

```
  </State>
```

*2 \- Then wait a second.*

```
  <State id="Wait1">
```

```
    <Transition afterTime="1">Bullet2</Transition>
```

```
  </State>
```

*3 \- Then shoot a different type of bullet, wait one second and start over.*

```
  <State id="Bullet2">
```

```
    <Behavior projectileId="1" cooldown="10.0">Shoot</Behavior>
```

```
    <Transition afterTime="1.0">Bullet1</Transition>
```

```
</State>
```

It will do this while following around any players or, if none are found, wandering aimlessly.

```
<Behavior bucket="movement">Follow</Behavior>
```

```
<Behavior bucket="movement">Wander</Behavior>
```

The state `<State id="Attacking">` is the main "behavior" for the test chicken. It will execute this loop until the conditions are met to transition to another state. This condition is defined at the end of the `<State id="Attacking">` :

```
<Transition hitpointsLessThan="0.5">Flash</Transition>
```

So, if the Test Chickens hitpoints are reduced below 50%, it will:

*4 \- Stop shooting, and flash for 1.0 second*

```
<State id="Flash">
```

```
  <Behavior type="Flash" color="FF0000">ShowEffect</Behavior>
```

```
  <Transition afterTime="1.0">Explode</Transition>
```

```
</State>
```

*5 \- It will then explode.*

```
<State id="Explode">
```

```
  <Behavior range="0.0" projectileId="0">Explode</Behavior>
```

```
</State>
```

During all of this, it will “Taunt” the player.

```
<Behavior>Taunt</Behavior>
```

### [Next: Orders \>](orders.md)

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
