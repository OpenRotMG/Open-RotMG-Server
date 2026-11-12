Source: https://sites.google.com/decagames.com/xml-documentation/xml-guide/behaviors-and-buckets

# Behaviors and Buckets

Behaviors are the basic unit of specifying how enemies act in the world. They have some rules that determine how they are executed.

Let's take the following as an example:

### The Black Bat Example

```
<Behavior bucket="movement">Charge</Behavior>
```

```
<Behavior bucket="movement">Wander</Behavior>
```

```
<Behavior range="1">Shoot</Behavior>
```

The "bucket" tag groups a set of Behaviors together in an ordered list.

For each "bucket", the Behaviors are executed in the order given and if any of the Behaviors succeeds, then the following behaviors are skipped.

In our *Black Bat* example, if "***Charge***" succeeds, then the monster will not "***Wander***". If "***Charge***" fails, then the monster will "***Wander***". The definition of "fail" depends on the particular Behavior but in this particular case, "fail" means that there was no player nearby to "***Charge***".

This has **no effect** on the "***Shoot***" behavior which is **always** executed because it is not part of any "bucket".

### [Next: States and Transitions \>](states-and-transitions.md)

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
