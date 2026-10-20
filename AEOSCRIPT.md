# AeoScript

**AeoScript** is Worldkiln's gameplay scripting language. It uses `.aeo` files to control game objects, input, events, cameras, animation, audio, and UI.

[Language documentation](https://aeowun.com/aeoscript/) · [API reference](https://aeowun.com/docs/reference/) · [Worldkiln](README.md)

## A game event

Create `scripts/score.aeo` in a Worldkiln project:

```aeoscript
score: number = 0

on coin_collected(points) {
    score += points
    debug.log("Score:", score)
}

event.fire("coin_collected", 10)
```

Start **Play** and open **Output** to inspect the result. The event handler receives a value, updates the score, and logs it.

## Variables and functions

Use `const` for bindings that cannot be reassigned. Mutable variables can be declared with an initializer or an explicit type; later assignments change their values.

```aeoscript
const BONUS = 5
total: number = 10

fn award(points) {
    total += points + BONUS
}

award(10)
```

AeoScript also has functions, closures, collections, control flow, and `wait()`. See [Syntax](https://aeowun.com/aeoscript/syntax/), [Functions](https://aeowun.com/aeoscript/functions/), and [Waiting](https://aeowun.com/aeoscript/waiting/) for their contracts.

## Scripts, Modules, and Entities

| Form | Use |
| --- | --- |
| Project script | Game-wide events, setup, and systems |
| Module | Reusable functions and state, including Actor-attached behavior |
| Entity script | Per-object fields and lifecycle functions |

Modules can be referenced from other scripts:

```aeoscript
const helper = scripts.game_helpers.aeo
```

An Entity declares state and lifecycle behavior for an object:

```aeoscript
entity Counter {
    count: number = 0

    fn update(dt) {
        count += 1
    }
}
```

See [Modules](https://aeowun.com/aeoscript/modules/) and [Entity scripts](https://aeowun.com/aeoscript/entities/) for project organization and attachment.

## Working with Worldkiln

Find an Actor by name:

```aeoscript
const door = proj.find("Door")

if door != nil {
    debug.log(door)
}
```

Worldkiln exposes APIs for [finding objects](https://aeowun.com/aeoscript/find/), [input](https://aeowun.com/aeoscript/input/), [UI](https://aeowun.com/aeoscript/ui/), [audio](https://aeowun.com/aeoscript/audio/), [events](https://aeowun.com/aeoscript/events/), and game systems in the [API reference](https://aeowun.com/docs/reference/).

For the supported object, Terrain, and Material APIs, consult the [API reference](https://aeowun.com/docs/reference/) for the exact function names and usage.

## Saving game state

Runtime changes normally reset when Play ends. Store persistent gameplay values with `save`:

```aeoscript
save.set("coins", 20)
save.commit()
```

Read them with `save.get("coins")`. See [Saving](https://aeowun.com/aeoscript/save/) for the save lifecycle.

## Reference

The [AeoScript language documentation](https://aeowun.com/aeoscript/) covers syntax and script organization. The [API reference](https://aeowun.com/docs/reference/) is the source for exact callable names, parameters, and return values. Script errors are covered in [Diagnostics](https://aeowun.com/aeoscript/diagnostics/).

