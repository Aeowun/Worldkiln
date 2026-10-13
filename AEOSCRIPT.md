# AeoScript Language Overview

> **Worldkiln:** v0.8.5
> **AeoScript:** v1.1.2  
> **Former engine name:** AeoEngine

AeoScript is Worldkiln's gameplay scripting language.

It can control gameplay objects, input, UI, audio, cameras, events, player behavior, and other game systems directly from `.aeo` files.

AeoScript supports three main scripting styles:

```text
Top-level scripts
    ↓
game systems, events, shared state, setup

Entity scripts
    ↓
object-specific state and lifecycle behavior

Modules
    ↓
reusable scripts and shared functionality
```

Learn more at [AEOWUN](https://aeowun.com).

---

# 1. Top-Level Scripts

AeoScript files can contain executable code directly at the top level.

```aeoscript
debug.log("Game script loaded")

gold: number = 0
max_enemies: number = 20
```

Top-level code runs when the script starts.

It is useful for:

- Game state.
- HUD logic.
- Spawning.
- Enemy orchestration.
- Coins and pickups.
- Traps.
- Events.
- World-wide gameplay systems.

A top-level script does not need an `entity` declaration.

```aeoscript
gold: number = 0

on on_gold(amount) {
    gold += amount
}
```

Variables declared at the top level remain available to functions and event handlers in the same script.

---

# 2. Script Modules

AeoScript files can be used as reusable modules.

Modules under `scripts/` are referenced through `scripts`:

```aeoscript
const helper = scripts.game_helpers.aeo

const result = helper.some_function(10)
```

Module functions use normal function arguments and return values:

```aeoscript
const math = scripts.math_helpers.aeo

const result = math.add(10, 20)
```

Modules can:

- Receive arguments.
- Return values.
- Call other modules.
- Use normal functions.
- Use closures.
- Use `wait()`.
- Listen for and fire events.
- Keep their own state.

Different module files keep separate state.

The same Module can also be attached to multiple Actors without those Actors sharing Module state.

PlayerScripts are available through:

```aeoscript
const controller = player.controller
```

and:

```aeoscript
const script = player.find("SomeScript")
```

Modules are general-purpose. They are not tied to a particular gameplay system.

---

# 3. Values

AeoScript's main value types are:

```text
number
bool
string
nil
basket
map
engine handles
functions
```

## Declaring and reassigning variables

**New variables require an explicit declaration.** Use `const` for a binding
that cannot be reassigned, or a name followed by a type annotation for a
mutable variable.

```aeoscript
const MAX_MOVE_SPEED = 5.0
waypoints: basket = [[10, 0, 10], [20, 0, -10]]
PLAYER_FOLLOW_DISTANCE: number = 5.0

PLAYER_FOLLOW_DISTANCE = 10.0
```

An assignment such as `PLAYER_FOLLOW_DISTANCE = 5.0` is **not** a declaration.
It is accepted only if `PLAYER_FOLLOW_DISTANCE` was already declared in an
accessible scope. Otherwise the semantic validator reports an undeclared
variable, and the runtime also rejects the assignment. Reassigning a `const`
binding is invalid.

This applies to top-level code, functions, event handlers, closures, and
nested blocks. Function parameters and `for` loop variables count as
declarations. Entity field declarations establish fields that entity methods
can subsequently update. Maps and other objects can still support their
documented dynamic properties; the rule concerns assigning a bare variable
name.

**Compatibility:** This declaration requirement describes the current
development language behavior. Older released editor builds may still accept
implicit top-level declarations until the next Worldkiln update.

## Numbers

Numbers are 64-bit floating-point values.

```aeoscript
health: number = 100
speed: number = 4.5
```

## Booleans

```aeoscript
enabled: bool = true
```

Boolean values are:

```text
true
false
```

## Strings

```aeoscript
name: string = "Ghost"
```

Strings support Unicode text.

## Nil

`nil` represents no value.

```aeoscript
const missing = nil
```

Missing Map keys return `nil`.

Assigning `nil` to a Map key removes that key.

---

# 4. Baskets

A `basket` is a zero-indexed sequence.

```aeoscript
const items = [1, 2, 3]
```

Baskets use reference semantics:

```aeoscript
const a = [1, 2]
const b = a

b[0] = 99
```

Both variables now refer to the same basket.

Use `basket.clone()` when a separate shallow copy is required.

Use `basket.freeze()` to make a basket read-only.

---

# 5. Maps

Maps store key/value pairs.

```aeoscript
const values = {}

values["name"] = "Ghost"
values[1] = "numeric key"
```

String and numeric keys are distinct.

Missing keys return `nil`.

Assigning `nil` removes a key.

---

# 6. Functions

Functions are declared with `fn`.

```aeoscript
fn add(a: number, b: number): number {
    return a + b
}
```

Functions can:

- Accept parameters.
- Return values.
- Call other functions.
- Be assigned to variables.
- Be passed as values.
- Be returned from other functions.
- Capture surrounding variables through closures.

---

# 7. Closures

Functions can capture state from the scope where they were created.

```aeoscript
fn make_counter() {
    count: number = 0

    return fn() {
        count += 1
        return count
    }
}
```

Closures can also be used for callbacks such as UI button actions and object events.

---

# 8. Control Flow

AeoScript supports:

- `if`
- `else if`
- `else`
- `while`
- `for`
- `return`

Example:

```aeoscript
health: number = 0
dead: bool = false
const items = ["sword", "shield"]

if health <= 0 {
    dead = true
}

for item in items {
    debug.log(item)
}
```

---

# 9. Events

Events allow scripts to communicate without directly calling each other.

Fire an event:

```aeoscript
event.fire("enemy_killed", enemy)
```

Receive it:

```aeoscript
on enemy_killed(enemy) {
    debug.log("Enemy defeated:", enemy.name())
}
```

Events can carry arguments.

---

# 10. Keyboard Input

AeoScript can react to individual keyboard presses and releases:

```aeoscript
on.keypress(key, down) {
    if key == "F" && down {
        attack()
    }
}
```

The `down` value indicates whether the key was pressed or released.

```text
press F    → keypress("F", true)
release F  → keypress("F", false)
```

Keyboard state can also be polled:

```aeoscript
if input.is_key_down("F") {
    ...
}
```

Common key names include:

```text
A
1
Space
ArrowUp
Escape
```

The engine reports the key. The script decides what that key does.

---

# 11. `wait()`

AeoScript code can pause and resume with `wait()`.

```aeoscript
debug.log("Starting")

wait(1.0)

debug.log("One second later")
```

The script continues from where it stopped.

`wait()` can be used inside functions, Modules, events, and other supported script execution.

```aeoscript
fn delayed_action() {
    wait(0.5)
    score += 10
}
```

---

# 12. Entity Scripts

An `entity` declaration is useful when behavior needs its own persistent per-object state.

```aeoscript
entity Counter {
    count: number = 0

    fn update(dt) {
        count += 1
    }
}
```

Multiple objects can use the same Entity script while keeping separate values.

Entity scripts are useful for:

- Object-specific state.
- Lifecycle functions.
- Reusable object behavior.
- Per-instance gameplay logic.

Common lifecycle functions include:

```aeoscript
fn on_spawn() { }
fn on_ready() { }
fn update(dt) { }
fn on_destroy() { }
```

Not every Entity script needs every lifecycle function.

---

# 13. Cells, Actors, and Entities

Worldkiln exposes several kinds of game objects to AeoScript.

## Cells

Cells are voxel World objects.

A specific Cell can be found by ID:

```aeoscript
const door = cell.get(12345)
```

Cells can expose properties and custom Attributes.

## Actors

Actors are project objects such as Characters, props, switches, items, and other more complex objects.

Actors have their own IDs and can have:

- Transforms.
- Packages.
- Physics settings.
- Attributes.
- Scripts.

Actors can be found through project queries:

```aeoscript
const switch_actor = proj.find("Switch_A")
```

Multiple Actors can share the same name.

Use:

```aeoscript
proj.find.all("Enemy")
```

when all matching Actors are needed.

Folder paths can narrow a search:

```aeoscript
const switch_actor = proj.Dungeon.find("Switch_A")
```

## Entities

An Entity represents a live game object during Play mode.

Entities can represent:

- The Player.
- Spawned characters.
- Actors currently in the game.
- Other game objects managed by Worldkiln.

Common Entity properties include:

```text
position
velocity
facing
grounded
animation
health
max_health
alive
is_actor
actor_id
```

Example:

```aeoscript
const enemy = get_entity("Goblin")

enemy.position = [10, 1, 5]
enemy.velocity = [0, 0, 1]
enemy.facing = [0, 1]
enemy.animation = "Walk"
```

Common methods include:

```aeoscript
enemy.jump()
enemy.damage(10)
enemy.heal(5)
enemy.destroy()
```

Check whether an Entity still exists with:

```aeoscript
enemy.is_valid()
```

---

# 14. Spawning Characters

AeoScript can spawn Characters during gameplay.

```aeoscript
const enemy = spawn("Goblin", [10, 1, 5])
```

Without a package argument, the default Character package is used.

A specific package can also be supplied:

```aeoscript
const enemy = spawn(
    "Knight",
    [15, 1, 10],
    "character_hero"
)
```

---

# 15. Finding Objects

Project Actors should generally be found through `proj`:

```aeoscript
const door = proj.find("Door")
```

Find every matching Actor:

```aeoscript
const enemies = proj.find.all("Enemy")
```

Find something inside a project folder:

```aeoscript
const switch_actor = proj.Dungeon.find("Switch_A")
```

`find(name)` also exists as a legacy global lookup:

```aeoscript
const things = find("Coin")
```

It is retained for compatibility and is planned for removal.

Player-attached Modules should be found through `player.find(...)`.

---

# 16. Cell Attributes

Cells can contain custom Attributes.

Supported values are:

- Number.
- Bool.
- String.

Example:

```aeoscript
const health = cell.attributes["health"]

cell.attributes["locked"] = true
```

Changes made to Cell Attributes during Play mode are temporary.

Assigning `nil` removes the temporary value and restores the saved value.

---

# 17. Play-Mode Changes

Ordinary script changes during Play mode do not rewrite the saved project.

For example:

```aeoscript
cell.visible = false
cell.solid = false
cell.color = [1, 0, 0]
```

These changes affect the active game session.

When Play mode ends, temporary changes are discarded.

Cells created with AeoScript during Play mode are also temporary unless an explicit project-editing operation says otherwise.

---

# 18. Collision Events

Scripts can react to physical contact.

The main touch callback is:

```aeoscript
fn on_touch(thing) {
    ...
}
```

`thing` identifies the object that caused the contact.

For example:

```aeoscript
coin.on_touch = fn(thing) {
    const player = get_entity("Player")

    if thing != player {
        return
    }

    coin.visible = false
}
```

This allows a script to distinguish the Player from enemies or other Entities.

Overlap callbacks use:

```aeoscript
fn on_overlap(overlapping, cell) {
    ...
}
```

---

# 19. Path Queries

Entities can request a path through the World:

```aeoscript
const path = enemy.find_path([20, 1, 20])
```

The result contains World-space waypoints.

AeoScript decides how the Entity follows those waypoints.

---

# 20. Gameplay Input

AeoScript can access common gameplay input such as:

- Movement.
- Jump.
- Mouse orbit.
- Cursor visibility.
- Mouse locking.
- Generic keyboard state.

Example:

```aeoscript
const move = input.get_move_vector()

if input.is_jump_pressed() {
    ...
}
```

---

# 21. Camera

Scripts can interact with the gameplay camera.

Available operations include:

- Camera position.
- Camera target.
- Camera orientation.
- Horizontal camera basis.
- Camera collision handling.

Example:

```aeoscript
const basis = camera.get_horizontal_basis()
```

---

# 22. Player Control

AeoScript can control supported Player behavior.

Current operations include:

- Horizontal velocity.
- Facing direction.
- Animation.
- Grounded state.
- Vertical impulse.
- Position.

Example:

```aeoscript
player.set_horizontal_velocity(2, 0)
player.set_facing_direction(1, 0)
player.select_animation("Walk")
```

Worldkiln continues handling Character physics and collision.

---

# 23. UI

Scripts can create UI controls:

```aeoscript
const panel = ui.new("Panel")
const text = ui.new("Text")
const button = ui.new("Button")
```

Current UI types include:

- `Panel`
- `Text`
- `Button`

Buttons can use function callbacks through `on_click`.

Viewport dimensions are available through:

```aeoscript
ui.get_viewport_size()
```

Editor-created UI can also be found by script key:

```aeoscript
const health = ui.find("health_text")
```

or:

```aeoscript
const health = ui.get("health_text")
```

A specific UI document can also be searched:

```aeoscript
const health = ui.find("hud", "health_text")
```

---

# 24. Audio

AudioEmitter Cells expose sound controls.

```aeoscript
const speaker = find("DoorSound")[0]

speaker.sound.play()
```

Current controls include:

- Play.
- Stop.
- Pause.
- Playing state.
- Looping.
- Volume.

---

# 25. Attachments

Objects can be attached to named Character rig points.

```aeoscript
attach(...)
```

and removed with:

```aeoscript
detach(...)
```

Attached objects follow the selected rig point as the Character moves and animates.

Their normal physics and movement return after detaching.

---

# 26. Animation Layers

One-shot animations can play over normal Character movement.

```aeoscript
entity.animation.play_layer(slot, clip, joint_root)
```

A layer can target part of a rig instead of replacing the Character's complete animation.

This is useful for actions such as attacks, item use, and other upper-body animations while movement continues.

---

# 27. Script Control

Scripts can enable and disable other scripts:

```aeoscript
script.disable("scripts/example.aeo")
script.enable("scripts/example.aeo")
```

Check state with:

```aeoscript
script.is_enabled("scripts/example.aeo")
```

Disabling a script stops its active execution for the current game session.

It does not delete the script file or its project binding.

---

# 28. Standard Library

Current built-in namespaces include:

```text
math
basket
string
get
input
camera
physics
player
ui
cell
entity
script
event
test
```

Common global functions and properties include:

```text
debug.log(...)
find(...)
getAllCellsOfClass(...)
get_entity(...)
spawn(...)
attach(...)
detach(...)
wait(...)
time.delta
```

See `AeoScript_API.md` and `AeoScript_STDLIB.md` for complete API references.

---

# 29. Engine Handles

AeoScript uses handles for objects owned by Worldkiln.

Current handle types include:

```text
Cell
Entity
Light
Sound
Ui
Mouse
```

Handles can become invalid when the object they refer to is removed.

Where needed, use APIs such as:

```aeoscript
entity.is_valid()
```

before continuing to operate on an object that may have been destroyed.

---

# 30. Errors and Diagnostics

AeoScript reports errors for problems such as:

- Invalid handles.
- Invalid arguments.
- Invalid collection access.
- Unsupported properties.
- Unsupported methods.
- Script execution failures.
- Execution-budget failures.

Diagnostics can include the script, function, source location, and related object context.

---

# 31. Language Direction

AeoScript is intended to remain:

- Small enough to learn.
- General enough for gameplay.
- Closely integrated with Worldkiln.
- Useful for both simple scripts and larger gameplay systems.
- Safe when working with engine-managed objects.
- Capable of pausing and resuming gameplay logic with `wait()`.

Top-level scripts remain the simplest way to create broad gameplay systems.

Entity scripts are available when behavior needs dedicated per-object state and lifecycle functions.

Modules provide reusable logic without requiring specialized APIs for every type of game system.

The language should grow around real Worldkiln workflows rather than accumulating APIs without a clear use.