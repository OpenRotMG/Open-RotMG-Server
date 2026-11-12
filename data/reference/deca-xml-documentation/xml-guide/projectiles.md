Source: https://sites.google.com/decagames.com/xml-documentation/xml-guide/projectiles

# Projectiles

Projectiles are also Objects and need to be created separately in order to be used by Monsters.

As we have seen in the [Tutorial](../getting-started/tutorial-your-first-monster.md) section, when we added our projectile to the monster we referred to the Yellow Star projectile by its ObjectId and defined how that projectile must behave for that specific monster. That means that the same projectile can be shot by different monster and have different properties!

Let's have a look at the example of creating a projectile as shown below:

### The Yellow Star Projectile Example

```
<Object type="0x0000" id="Yellow Star">
```

```
    <Class>Projectile</Class>
```

```
    <Texture><File>lofiObj</File><Index>0x76</Index></Texture>
```

```
    <Rotation>100</Rotation>
```

```
</Object>
```

![](../images/8fafaea32db39f908b1d.png)

*Yellow Star Sprite*

Here is what each of the fields mean:

```
<Object type="0x001a" id="Yellow Star">
```

This creates the concept of an object. The “type” and “id” fields must be unique. For projectiles, type must be less than 0xFF. For testing purposes, use the 0x005X space for projectile types and use whatever ids you want (although, again, they need to be unique).

```
<Class>Projectile</Class>
```

This is a boilerplate used internally, always include it.

```
<Texture><File>lofiObj</File><Index>0x76</Index></Texture>
```

This is the texture to use for the projectile.

```
<Rotation>100</Rotation>
```

This will cause the projectile to rotate as it moves. The default (0) is no rotation.

```
       <AngleCorrection>1</AngleCorrection>
```

If the texture points toward the upper right hand corner (45°) instead of to the right (0°), include this line to ensure that the projectile is correctly rendered.

### [Next: Condition Effects \>](condition-effects.md)

[**Leave feedback**](../feedback.md)

Is something described here not clear or do you have ideas on how to improve the documentation? Let us know!
