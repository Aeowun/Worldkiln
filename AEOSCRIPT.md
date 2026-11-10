# AeoScript
You can view documentation and search API and other rules online.
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

Use `const` for bindings that do not change. Mutable variables are declared with a name and type; plain assignment updates an existing variable.

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

Newer **BETA** APIs include Actor tags, property-change observers, Terrain surface queries, and selected Material animations.

## Saving game state

Runtime changes normally reset when Play ends. Store persistent gameplay values with `save`:

```aeoscript
save.set("coins", 20)
save.commit()
```

Read them with `save.get("coins")`. See [Saving](https://aeowun.com/aeoscript/save/) for the save lifecycle.

## Reference

The [AeoScript language documentation](https://aeowun.com/aeoscript/) covers syntax and script organization. The [API reference](https://aeowun.com/docs/reference/) is the source for exact callable names, parameters, and return values. Script errors are covered in [Diagnostics](https://aeowun.com/aeoscript/diagnostics/).

## Game lifecycle

Use the `game` namespace to control the current game instance, not unrelated windows or processes.

| API | Behavior |
| --- | --- |
| `game.quit()` | Signals `game_on_quit`, then exits the calling game instance (or leaves Editor Play). |
| `game.restart()` | Signals `game_on_quit`, then reloads the running game in its existing window, preserving saved values. |
| `game.pause()` | Pauses local single-player simulation. Warns and does nothing in multiplayer. |
| `game.resume()` | Resumes local single-player simulation. Warns and does nothing in multiplayer. |
| `game.is_paused` | Boolean indicating whether gameplay is paused. |
| `game.elapsed_time` | Seconds of active gameplay since this session began. |
| `game.delta_time` | Seconds advanced by the latest gameplay update, zero while paused. |
| `game.is_editor` | True during Editor Play. |
| `game.is_standalone` | True when running in a standalone game window. |
| `game.on_quit` | Event name `game_on_quit`. |
| `game.on_pause` | Event name `game_on_pause`. |
| `game.on_resume` | Event name `game_on_resume`. |

Use existing AeoScript named event handlers for lifecycle notifications:

```aeoscript
on game_on_quit() {
    save.commit()
}
```

`game_on_quit` is only a signal. Saving and other end-of-game actions are entirely creator-authored. A game may decide when to call `game.quit()` based on its own save logic.

`game.restart()` reloads the current gameplay session without closing its window and without clearing saved values. `save.restart()` only resets and commits saved values; it does not restart gameplay. `save.reset()` retains its existing uncommitted reset behavior.

A lost multiplayer host does not terminate a client process. Reliable transfer of host authority is a separate networking operation and is not provided by `game.quit()`.

## Internet multiplayer invitations (V1.1.0 development)

Internet sessions are opt-in. A project may use the general AeoScript networking calls to create and join sessions.

| Call | Behavior |
| --- | --- |
| `network.host_invite(2)` | Asynchronously requests a relay-backed two-player session; accepts an optional player capacity from 2 to 8. |
| `network.join_invite(link)` | Asynchronously joins a session by HTTPS invite URL or 32-character invitation code. |
| `network.invite_link()` | Returns the current host's shareable URL, or an empty string when none is active. |
| `network.leave()` | Leaves the active session and returns the calling game instance to Solo. |
| `clipboard.write(link)` | Copies a string to the system clipboard; returns true on success. |
| `clipboard.read()` | Reads text from the system clipboard. |

A host receives `network_invite_ready(link)` when the relay issues an invitation; a failed connection emits `network_join_failed(reason)`. The existing `network_started` and `network_disconnected` events remain available. The game decides if, when and how these events affect its authored UI.

```aeoscript
on network_invite_ready(link) {
    clipboard.write(link)
    debug.log("Invitation copied")
}

on network_join_failed(reason) {
    debug.log("Unable to join:", reason)
}
```

The default relay uses outbound encrypted WebSocket connections. A custom HTTPS relay origin can be configured with the runtime environment variable `WORLDKILN_RELAY_URL`. The relay is transport only; host-authoritative gameplay and scene replication remain in the existing game runtime.

