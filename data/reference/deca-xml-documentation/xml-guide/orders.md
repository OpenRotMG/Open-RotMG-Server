Source: https://sites.google.com/decagames.com/xml-documentation/xml-guide/orders

# Orders

Sometimes it is useful to have the behaviors of a bunch of monsters coordinated. To do this, you can have a single monster give "***Orders***" to the other monsters.

Here is another example to illustrate "***Orders***":

### The Deathmage Example

```
<State id="Waiting">
```

```
    <Order>
```

```
        <Behavior protecteeId="Deathmage">Protect</Behavior>
```

```
    </Order>
```

```
    <Transition playerWithin="15">Attacking</Transition>
```

```
</State>
```

```
<State id="Attacking">
```

```
    <State id="Circling">
```

```
        <Order>
```

```
            <Behavior>Circle</Behavior>
```

```
        </Order>
```

```
        <Transition afterTime="2.0">Engaging</Transition>
```

```
        </State>
```

```
    <State id="Engaging">
```

```
        <Order>
```

```
            <Behavior bucket="movement" range="1" speed="1.0" acquireRange="15">Follow</Behavior>
```

```
        </Order>
```

```
    </State>
```

```
    <Transition noPlayerWithin="30">Waiting</Transition>
```

```
</State>
```

Then, a Skeleton has the following XML:

```
<Behavior projectileId="0" range="3">Shoot</Behavior>
```

```
<Behavior bucket="movement" listenIds="Deathmage">FollowOrders</Behavior>
```

```
<Behavior bucket="movement" range="1" speed="1">Follow</Behavior>
```

```
<Behavior bucket="movement" speed="0.4">Wander</Behavior>
```

So, if a Skeleton finds a Deathmage nearby, it will activate the "***FollowOrders***" behavior.

This means that it will take whatever is specified in the Order tag for the current state of the Deathmage and executes those Behaviors as normal. The Order tag can have sub states as normal.

For the example above, the Skeleton will “Protect” (i.e. stay close to) the Deathmage until a player comes near.

```
<Order>
```

```
<Behavior protecteeId="Deathmage">Protect</Behavior>
```

```
</Order>
```

The Deathmage will then transition to the “Attacking/Circling” state. In this state, the Skeleton (and all other Skeletons and anyone else listening to the Deathmage’s orders) will follow the order to Circle which will cause him to circle around the player.

```
<Order>
```

```
<Behavior>Circle</Behavior>
```

```
</Order>
```

Then, 2 seconds later, the Deathmage will transition to the “Attacking/Engaging” state. This will cause the followers to move toward the player and their “Shoot” behavior will cause them to attack. This is a basic “ambush” behavior.

```
<Order>
```

```
<Behavior bucket="movement" range="1" speed="1.0"
```

```
acquireRange="15">Follow</Behavior>
```

```
</Order>
```

If the Deathmage is killed, the FollowOrders behavior will fail and the Skeleton will fall back to Follow or, failing that, Wander.

It is also possible for an Object to have different orders for different Object types. Here is an example:

```
<Order targetIds="Skeleton,Skeleton Veteran">
```

```
<Behavior>Circle</Behavior>
```

```
</Order>
```

```
<Order targetIds="Skeleton Mage">
```

```
<Behavior>Protect</Behavior>
```

```
</Order>
```

The idea here is that the Skeletons and Skeleton Veterans will ambush and the Skeleton Mages will stay close to the Deathmage and continue to protect him!

Note that Order has the full functionality of a State so you can have complex behaviors specified with control flow both via bucket and states and transitions.

For example:

```
<Order>
```

```
<State id="Bullet1">
```

```
<Behavior numShots="3" projectileId="0"
```

```
cooldown="0.6">Shoot</Behavior>
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
<Behavior projectileId="1"
```

```
cooldown="10.0">Shoot</Behavior>
```

```
<Transition afterTime="1.0">Bullet1</Transition>
```

```
</State>
```

```
</Order>
```

This will cause the Skeletons to do the behavior described above for the previous "Test Chicken" (assuming they have two projectiles specified).

### [Next: Tags \>](tags.md)

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
