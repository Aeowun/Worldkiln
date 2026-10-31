<!--
Worldkiln
Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
Licensed under the AeoEngine Source-Available License.
See LICENSE in the repository root for the complete terms.
-->
# AeoScript runtime authoring reference

## 1. Fast start: syntax

<!-- AI-LANGUAGE-CONTRACT:BEGIN -->

~~~aeoscript
// Literals and collections
const count = 10
const pi = 3.14
const enabled = true
const absent = nil
const label = "Ghost"
const positions = [1, 2, 3]

// Mutable declaration: an explicit type annotation is required.
speed: number = 4
speed = 5
speed += 1
target: Entity? = nil

// Constants cannot be reassigned.
const max_speed: number = 12

// Named function, typed parameters/return, and closure.
fn add(a: number, b: number): number {
    return a + b
}
const double = fn(x) {
    return x * 2
}

// Branches and loops
if enabled {
    print("On")
} else if absent == nil {
    print("Missing")
} else {
    print("Off")
}

for item in positions {
    debug.log(item)
}

while speed > 0 {
    speed -= 1
}

const first = positions[0]
const who = state["name"]
~~~

**Critical language rules:**

- **Declarations are explicit.** Use a typed declaration (for example, speed: number = 4) or const. A first-time bare assignment such as speed = 4 is NOT a declaration; assigning an undeclared variable errors. Avoid the old 0.7.x pattern.
- Use **fn** for functions; **const** for immutable bindings; **nil**, **true**, and **false** as literal values. The language has no var/let declarations.
- Use newlines and blocks to separate statements. **Semicolons are not supported** as statement separators.
- The parser accepts trailing commas in argument lists, basket literals, map literals and parameter lists, and newline continuation in expressions.
- Assignment operators: =, +=, -=, *=, /=. Targets: variable, property, collection index; arbitrary expressions are invalid assignment targets.
- Operators: !, unary -, *, /, %, +, -, <, <=, >, >=, ==, !=, &&, ||. Member/call/index bind more tightly than unary, followed by arithmetic, comparison, equality, AND, OR.
- Types are annotations, not full static typing: number, bool, string, named engine types and optional suffix ?. Dotted names such as ui.Panel? parse as types but do not by themselves guarantee runtime availability.
- Identifier map keys are strings. Number and string map keys differ. A missing key evaluates to nil; assigning nil to a map key removes it.
- Line comments use //; the current grammar does not define block comments.

Top-level statements run when Play starts; a function declaration does not call it. Independent project scripts need no Actor attachment. Actor Modules use `self.parent` after actual attachment. Legacy Cell-bound entities have their own lifecycle; do not invent startup callbacks for ordinary scripts. The active `player` may be `nil`: check it before accessing properties. Never substitute a named lookup for active-player identity. Static validation does not prove saved changes or Play behavior.

<!-- AI-LANGUAGE-CONTRACT:END -->

### Functions, entity definitions, and event declarations

~~~aeoscript
entity Counter {
    hits: number = 0

    fn on_spawn() { }
    fn on_ready() { }

    fn update(dt) {
        hits += 1
    }

    fn on_touch(other) {
        debug.log(other)
    }

    fn on_destroy() { }
}

// Namespaced input event; down is a bool.
on.keypress(key, down) {
    if key == "F" && down {
        debug.log("Use")
    }
}

// Named custom event.
on quest_complete(item) {
    debug.log(item)
}
~~~

**Scope warning:** automatic Entity lifecycle methods (especially update(dt)) apply to **legacy Cell-bound Entity scripts**. Do not assume a script stored in scripts/ or an Actor/Player module gets update(dt) automatically. Actor and Player modules use their actual module, event, or controller invocation path.

### Values, truthiness, and indexing

| Kind | Example | Condition behavior |
| --- | --- | --- |
| Boolean | true / false | Direct true/false |
| Nil | nil | False |
| Engine handle | Actor / Entity / Cell / Light / Ui / Sound | True when present; check validity for stale handles |
| Script object | entity instance | True |
| Number, string, basket, map, function, namespace | 0 / "" / [] / {} | **Not legal as conditions**; use an explicit comparison instead |

- Baskets are zero-indexed, mutable reference-backed sequences. Aliases share elements; basket.clone makes a separate outer basket.
- Indices must be representable, non-negative integers. Out-of-range accesses/mutations may fail; do not rely on silent clamping.
- Maps accept numeric or string keys; other map key kinds are rejected.
- Supported engine handles: Actor, Entity, EntityPhysics, EntityAppearance, EntityAnimation, Light, Cell, Ui, Sound, Mouse.
- A handle is an ID-based runtime reference, not ownership of the underlying Rust object.

---

## 2. Script files, modules, lookup, and lifecycle

**Script locations:** scripts/ for ordinary modules, playerscripts/ for player-attached modules. Imports have a parser form such as import scripts.systems.health.aeo. Direct module lookup is common:

~~~aeoscript
const helper = scripts.game_helpers.aeo
const answer = helper.calculate(7)

// In an actor-attached/player module:
const actor = self.parent
const controller = player.controller
const player_module = player.find("inventory")
~~~

- The runtime caches a module instance per canonical script path, with persistent module top-level state for that Play runtime. A standalone module does not automatically share the Actor/Player's self.parent.
- self.parent resolves to the parent **Actor** for actor-attached/player modules. Accessing it on an unattached module raises an error.
- Script bindings attach by authored Cell ID / Actor ID, **not by displayed object name**.
- Module functions and closures may retain lexical state. Functions can call other modules.
- Runtime mutations (Cells, Actors, UI and audio overrides) are **not** automatically saved to the authored project. Use the save API separately for supported primitive gameplay state.

**AI Module diagnostics and source retrieval [release-compiled; live Play unverified]:** Isolated AeoScript static checks show unresolved Module-import references as unverified warnings rather than hiding them or falsely rejecting valid project imports. The editor final validate/write/attach boundary resolves imported `.aeo` Modules against the active project scripts/playerscripts listing; missing imports and unavailable/truncated project indexes reject the operation, preserving source. Automatic context includes more natural-language scripting intents and bounded native host dispatch excerpts, identified with build commit and source fingerprint. Read-only script reports use the active project import index, as approved writes do; smaller-model and live Play verification remain pending.

### Preferred discovery

~~~aeoscript
const gate = proj.find("Gate")          // one Actor or nil
const all_gates = proj.find.all("Gate")  // basket of Actors

const dungeon = proj.Dungeon            // named project folder
const chest = dungeon.find("Chest")      // scoped search
const guards = dungeon.find.all("Guard")

const running_ghost = get_entity("Ghost")  // legacy global
const entity_by_id = entity.get(23)
const cell_by_id = cell.get(10)
~~~

| API | Result | Notes |
| --- | --- | --- |
| proj.find(name) | Actor / nil | Authored project Actor discovery; names may repeat |
| proj.find.all(name) | basket | All matching Actors |
| folder.find(name), folder.find.all(name) | Actor / nil, basket | Searches the folder hierarchy |
| player.find(name), player.find.all(name) | Module / nil, collection | Player-attached modules |
| scripts.path.to.module.aeo | Module reference | Reusable standalone modules |
| entity.get(id), entity.exists(id) | Entity / nil, bool | Runtime Entity IDs |
| cell.get(id), cell.exists(id) | Cell / nil, bool | Effective runtime Cells |
| get_entity(name) | Entity / nil | Compatibility global |
| get_light(x, y, z) | Light / nil | Compatibility global |
| getAllCellsOfClass(class_name) | basket | Legacy Cell-class query |
| find(name) | basket | Legacy mixed-object lookup; prefer scoped lookup |

**Avoid using name matches as stable IDs.** The same name can exist on multiple Actors/Entities.

### Lifecycle / events

| Callback/event | When |
| --- | --- |
| fn on_spawn() | Legacy Cell-bound Entity instance creation |
| fn on_ready() | Supported scripted instance ready stage |
| fn update(dt) | Per-frame on legacy Cell-bound Entity scripts only |
| fn on_touch(other) | Supported collision/contact **entry** |
| fn on_overlap(overlapping, cell) | Supported overlap entry/exit; overlapping is boolean |
| fn on_destroy() | Supported runtime script destruction |
| on navigation_finished(actor) | Navigation completed |
| on navigation_failed(actor, reason) | No path / navigation failure |
| on.level_loaded() | One event after scene/controller initialization |
| on.entity_contact(a, b) | Repeats each physics update during Entity-to-Entity contact |
| on.player_entity_contact(other) | Repeating physical Player contact only |
| on.keypress(key, down) | Physical key press/release |
| on.mousepress(button, down) | Mouse button press/release: Left, Right, Middle |
| on custom_event(...) | Event fired with event.fire or matching engine event |
| on animation_event(payload) | Animation event frame; payload map |

**Contacts:** on_touch is not a sustained-contact damage loop. Repeating contact events require your own hit-window or cooldown gating. Gameplay mouse presses consumed by UI do not reach gameplay scripts.


### Fibers / waiting / script control

~~~aeoscript
wait(0.25)                      // yield this fiber; standalone statement

script.enable("scripts/game.aeo")
script.disable("scripts/game.aeo")
const is_on = script.is_enabled("scripts/game.aeo")

event.fire("quest_complete", "Old Flute")
~~~

- wait(seconds): **exactly one finite positive number**, as a standalone statement. It suspends the current fiber and preserves call frames, closures, locals, instruction location and loop state.
- Disabling a script cancels its active fibers and its registered observers. It does not delete the script file.
- Standard execution guardrails include 100,000 operations per budget and 64 call-depth levels; do not write unbounded synchronous loops.
- time.delta is the script-host frame delta time. **Do not** treat editor outside-file reload as live Play-mode hot-swapping.

---

## 3. World, Actors, Entities, and Cells

**Editor Actor picking and pivots [development; live verification pending]:** In authoring mode, Actor selection tests rendered mesh triangles instead of Actor physics Box colliders or empty model bounds. Collision-disabled Actors remain selectable if visible geometry exists; Actors without rendered faces are selected through Explorer. Editor gizmos use the visible model center and X/Y/Z world-axis rotation about that center, without changing the AeoScript transform or collision API. Directional shadows cover 3× the prior camera range in Editor authoring only; Play/game lighting stays unchanged. The experimental terrain PCF and enlarged world-shadow filtering offsets were reverted after visible shading artifacts. Existing shadow sampling behavior is restored in source; live release-editor comparison remains pending.

**Opt-in skeletal pose budget [source-corrected; Play unverified]:** New worlds now start with the Animation Budget Allocator disabled. The existing Project Settings toggle may enable it; persisted choices in project world files are authoritative, including previously saved enabled settings. It targets optional skeletal visual poses, not script time, events, physics, or movement. Current prioritization depends on active-player distance; without an active player all Characters count as near, and camera visibility and attachment significance are not yet integrated. Real-world visual/attachment acceptance is pending.

### Runtime spawn and attachments

~~~aeoscript
// Entity/Character declaration or package must resolve in active host.
const ghost = spawn("Ghost", [8, 2, 4])
// Third spawn argument may select a real installed character package.

// Attach named points. "Origin" is available for a plain prop child.
const hero = proj.find("Hero")
if hero != nil {
    attach(ghost, "Origin", hero, "head", [0, 1, 0])
}
const detached = detach(ghost)
~~~

- spawn(entity_name, [x, y, z] [, character_package]) returns a runtime Entity handle. It can fail for invalid names or unavailable host/package.
- attach(child, child_point, parent, parent_point [, [x, y, z]]) takes Entity or Actor handles; offset is in the parent attachment's local coordinates.
- detach(child) returns whether the child was attached.

### Actor handles (authored project objects)

~~~aeoscript
const guard = proj.find("Guard")

if guard != nil && guard.is_valid() {
    guard.set_position(10, 2, 8)       // three number arguments
    guard.translate(0, 0, 2)
    guard.set_horizontal_velocity(2, 0)
    guard.set_facing_direction(0, 1)
    guard.select_animation("Walk")
    guard.navigate_to([20, 2, 8], 3.5, 0.25)
}

on navigation_finished(actor) {
    debug.log("Reached destination", actor.name)
}
on navigation_failed(actor, reason) {
    debug.log("Navigation failed", reason)
}
~~~

**Actor properties (current host):** id, actor_id, is_actor, name, position, facing, rotation, scale, visible, enabled, package, attributes, physics, animation, can_collide, solid, collision_enabled, anchored.

**Actor methods:**

| Method | Behavior |
| --- | --- |
| is_valid() / name() | Liveness / object name |
| set_position(x, y, z) / translate(dx, dy, dz) | Teleport / relative move |
| set_horizontal_velocity(vx, vz) / set_facing_direction(dx, dz) | Character movement/facing |
| select_animation(name) | Select imported or project-owned Character clip by name |
| is_grounded() / apply_vertical_impulse(impulse) | Character movement state |
| navigate_to([x, y, z] [, speed [, stop_distance]]) | Start native Character navigation |
| is_navigating() / cancel_navigation() | Check/cancel navigation |
| damage(amount) | Emit game-neutral on.damage(target, amount) event; no engine HP |
| destroy() | Remove runtime representation, not authored project object |

**Navigation:** default speed = 4.0 and stop distance = 0.1. A path failure returns false and emits navigation_failed with the Actor and reason; invalid arguments or unsupported hosts can raise errors. Native navigation handles route-following and collision, rather than script-side interpolation.

**Editor-only viewport selection (no AeoScript API change):** With Select mode active, dragging from empty viewport space draws a temporary rectangular marquee and selects visible authored Cells and Actors in its screen projection. The editor reuses its existing multi-selection/Project Explorer state, with no World mutation, scripted event or new Play-mode input handling. Move/Rotate/Scale and other authoring tools retain their existing input ownership.


**Nonblocking runtime builds [BETA, compile-checked]:** Cold or missing Character profiles and dirty-tile replacements queue native background generation. `navigate_to(...)` returns `true` when the request is accepted, including while its mesh is pending; `is_navigating()` remains `true` during that wait. The existing native follower starts automatically when a validated build is ready. Cancellation and newer requests release or replace pending goals. A later build/query failure uses the existing `navigation_failed` event and reason codes. `find_path(...)` keeps its route-or-`nil` shape and returns `nil` while data is pending; a later query can use the completed cache. No new script methods or editor controls are introduced. Radius and height round upward to stable 1/32-stud buckets, so similar capsules reuse a conservative bake. Cell indexing advances with a one-millisecond frame budget; collision triangles are captured for at most one navigation tile per scene frame, with shared source chunks, static colliders, and cached terrain tiles. Rasterization, polygon generation, and portal stitching run on one worker without an engine-thread wait. A collision-tile capture is an indivisible preparation step and can exceed the Cell-indexing budget. Runtime smoothness and city-scale capacity have not been measured for this revision; existing safety limits remain. See [Navigation implementation and limits](../../../reference/Worldkiln-Navigation.md).

**Character stopping range [BETA]:** `navigate_to` searches for a reachable, clearance-valid polygon position within `stop_distance` of the requested target, including wall-adjacent positions a larger Character cannot occupy exactly. Completion is measured from the original target. `find_path` retains its exact-target checks; stopping range never permits routes outside bounds, through walls, or across disconnected floors. Native Character collision remains authoritative.

**Active Character collision [BETA]:** Character-capable authored Actors use their authored Box collider, including older Auto-mode records, with Actor scale applied once to both physics and the existing green wireframe. No collision disables the authored body without restoring a hidden package capsule. Navigation clearance uses a conservative profile enclosing that selected body; it is not an additional solid collider. Character separation and Actor contact corrections recheck Cell walls and terrain, preventing pushes through walls. Built-in Characters use a Box derived from their Character dimensions. Focused native movement and collision regressions are verified; owner live Play/FPS acceptance remains pending.

**Actor-specific callbacks:** assign actor.on_navigation_finished = fn() { ... } or actor.on_navigation_failed = fn(reason) { ... } for callbacks scoped to that Actor.

**Physics flags:** can_collide and solid are aliases for solid physical response. collision_enabled separately enables physics collision; anchored prevents simulation movement and clears accumulated force/velocity. Actor physics settings refer to the same underlying Entity physics, not a separate script collider. Character-only operations reject incompatible Actors. Actors have no built-in or healing. The game handles damage events using ordinary script state.

**Destroy is not authoring deletion.** Destroying an Actor is a Play-session operation; use save.set/save.commit if persistence of a gameplay outcome is required.

### Runtime Entity handles

**Properties:** id, name, position, velocity, facing, grounded, animation_name, animation, physics, appearance, is_actor, actor_id, package, attributes, sound.

**Methods:** is_valid(), name(), set_position(x,y,z), translate(dx,dy,dz), jump([impulse]), damage(amount), destroy(), find_path([x,y,z]), navigate_to([x,y,z] [, speed [, stop_distance]]), is_navigating(), cancel_navigation().

~~~aeoscript
const enemy = get_entity("Ghost")
if enemy != nil && enemy.is_valid() {
    const path = enemy.find_path([24, 1, 8])  // basket of waypoints or nil
    if path != nil {
        enemy.navigate_to([24, 1, 8], 2.5, 0.5)
    }
    enemy.animation.play_layer("attack", "Sword Attack", "spine")
}
~~~

- Native Entity navigation works for Character-backed runtime Entities, including spawned Characters. No path returns nil from find_path; navigate_to may return false and dispatch navigation_failed.
- Entity physics: entity.physics.enabled, solid, anchored, collision_enabled, mass, velocity, gravity_scale, size; read-only entity, kind and grounded. Methods: is_valid, apply_force([x,y,z]), apply_impulse([x,y,z]), stop, wake.
- Entity animation: entity.animation.play_layer(slot, clip, joint_root), returning true on success. Slot, clip and joint_root are required strings; clip name must exist for the active rig.
- Entity appearance: currently a capability foundation (entity/kind/is_valid), **not** a general rendering-property editor.
- Imported and Animation Editor clips are selected by animation **Name**. Matching project-owned clip names can override clips for the corresponding rig.

### Cells and lights (world/grid/runtime)

~~~aeoscript
const wall = cell.new("Block")
wall.position = [0, 2, 0]
wall.solid = true
wall.visible = true
wall.attributes["owner"] = "TeamA"
wall.attributes["owner"] = nil  // removes runtime override

const lamp = cell.new("Light")
lamp.enabled = true

cell.delete(wall)
~~~

**cell.new supported runtime types:** Block, FxBlock, Player, Light, SpawnPoint, AudioEmitter, ParticleEmitter. **NPC is not a supported cell.new type** in the present host.

**Cell/Light properties:** id, name, cellType, position, visible, enabled, solid, anchored, color, offset, attributes, sound where applicable. Methods include cell.getObject(), cell.is_enabled(), cell.set_enabled(bool); host support is handle-dependent.

**Cell namespace:** cell.new(type), cell.delete(handle), cell.get(id), cell.exists(id).

**Attribute values:** number, bool, string; nil clears a runtime override. Actor and Entity attributes also use their host-backed attribute container. Runtime attribute changes do not rewrite authored data.

### World classification, [BETA] tags and observation

~~~aeoscript
const doors = tags.get("door")            // Actor / Cell / Light handles
const enemies = tags.find("enemy")        // Actor-only compatibility search
const gate = proj.find("Gate")
if gate != nil && tags.has(gate, "door") {
    tags.add(gate, "opened")             // runtime override
    tags.remove(gate, "locked")
}

fn changed(before, after) {
    debug.log("Visibility", before, after)
}

if gate != nil {
    const token = observe.property(gate, "visible", changed)
    observe.cancel(token)
}
~~~

- tags.get(tag) returns all matching live Actors/Cells/Lights. tags.find(tag) remains Actor-only. tags.has/add/remove(handle, tag) return booleans. Tag overrides last for Play only.
- Tags are case-sensitive, **1–64 UTF-8 bytes**, no leading/trailing whitespace or control characters.
- observe.property(handle, property, callback) returns a numeric subscription ID, with (old_value, new_value) delivered on the next script update after a detected change. observe.cancel(id) cancels it.
- Observation supports live Actor, Cell, Light and Entity readable properties. No initial callback; observers are bounded and polling-based. It does not observe every intermediate physics/GPU state. Disabling the owner script or destroying the target cancels its subscription.

---

## 4. Player, input, mouse, camera, physics query

**Player logic selection [development; Play/export unverified]:** If a Player Character is selected, its saved Controller and Camera references take precedence over project-level choices in Editor Play and standalone runtime. Missing or invalid Character metadata falls back to project selections. Without a selected Character, no Player-specific controller or camera script is selected automatically; projects can still assign cameras explicitly.

### Script-owned movement and physics [BETA; live Play verification pending]

General-purpose, **opt-in** movement controls are available to every project; no ladder behavior is built into Worldkiln, and tags alone never start movement. An active Player can temporarily hand off its installed controller while the native Character physics and collision solvers continue:

~~~aeoscript
const old_gravity = player.get_gravity_scale()
const old_controller = player.is_controller_enabled()
player.set_controller_enabled(false)
player.set_gravity_scale(0.0)
player.set_velocity(0.0, 2.5, 0.0) // climb, fly, animate, or simulate at chosen speed
// On exit, interruption, level change or removal:
player.set_velocity(0.0, 0.0, 0.0)
player.set_gravity_scale(old_gravity)
player.set_controller_enabled(old_controller)
~~~

- `player.set_velocity(vx, vy, vz)` and `player.get_velocity()`: full world-space velocity; getters return `[x,y,z]` or `nil` without an active Player.
- `player.set_gravity_scale(number)` and `player.get_gravity_scale()`: per-character gravity multiplier, 1.0 = authored project gravity, 0.0 = weightless, accepted finite range -100…100; does not change world gravity.
- `player.set_controller_enabled(bool)` and `player.is_controller_enabled()`: explicit active-player input-controller handoff. Controller disabled **does not** turn off gravity, collisions, simulation or animation; those remain native.
- Runtime `entity.physics.velocity` and `entity.physics.gravity_scale` now affect a Character-backed Entity through the same CharacterSystem as ordinary non-Character physics; default behavior is unchanged until a script writes them.


~~~aeoscript
const move = input.get_move_vector()     // [x, y]
const jumping = input.is_jump_pressed()
const orbit = input.get_orbit_delta()    // [dx, dy]
const use_held = input.is_key_down("F")  // bool

const pos = player.position              // [x, y, z]
const grounded = player.is_grounded()
const swimming = player.is_swimming()
player.set_horizontal_velocity(2, 0)
player.set_facing_direction(1, 0)
player.select_animation("Walk")
player.apply_vertical_impulse(4)

const basis = camera.get_horizontal_basis() // [forward[3], right[3]]
const current = camera.get_script()
camera.set_script("firstPerson")
camera.set_position([0, 4, 8])         // or x, y, z
camera.set_target([0, 1, 0])           // or x, y, z
camera.set_orientation(0, 0)           // yaw, pitch
const safe = physics.resolve_camera_collision([0, 1, 0], [0, 4, 8])

// Native SpringArm, opt-in on each camera update; any target point.
const boom = camera.spring_arm({
    target: [0, 1, 0],
    offset: [0, 2, 6],
    dt: dt,
    follow_speed: 8.0,
    rotation_speed: 12.0,
    recovery_speed: 10.0,
    look_ahead: [0, 0, 0],
    dead_zone: [0.08, 0.08],
    collision: true,
    collision_padding: 0.1
})
camera.set_script(current)

const mouse = get.mouse()
mouse.setCursorVisible = true
mouse.setScreenLocked = false
~~~

- Key names include A–Z, 0–9, Space, Escape and ArrowUp/Down/Left/Right. input.is_key_down and on.keypress use the same physical key naming.
- Engine suppresses automatic key repeat in keypress events; focus loss releases held keys.
- on.mousepress(button, down) receives Left/Right/Middle. The pointer may be consumed by UI.

**Input ownership [development; Play unverified]:** Editor text focus blocks new gameplay key presses and `input.is_key_down` entries; a prior gameplay-owned press still receives a release when the key is lifted or the window loses focus. Standalone Escape is script-available rather than an automatic quit command, and right-click no longer forces cursor locking. Standalone mouse presses accepted by gameplay receive matching releases, including after pointer focus changes; Editor Play currently tracks this cross-UI capture for the left button. Physical gamepad and touch support are outside this correction.
- camera.get_script / set_script select the current installed camera package during Play only; they do not change Project Settings or Player controller. Unknown packages report errors.
- physics.resolve_camera_collision(target, desired) expects **two [x,y,z] baskets** and returns the resolved three-number position.

### Spatial raycasting and line-of-sight [BETA]

Scripted shooters, AI visibility checks, procedural samplers and custom camera systems can call `physics.raycast(origin, direction, distance [, options])`, `physics.raycast_all(...)`, `physics.linecast(start, end [, options])`, `physics.linecast_all(...)` or `physics.line_of_sight(...)`. Queries return the closest actual collision surface and its world-space position, normal, distance and object handle, or a sorted multi-hit basket. Per-call options select Actors, Entities, Cells and terrain, ignore handles and bound hit count. No engine-wide Actor hiding, camera push, transparency or gameplay response is implied. Source-implemented; live Play verification pending. See [Physics Query](#16-physics-query) and [Spatial Queries](../../../reference/Worldkiln-Spatial-Queries.md).

### Native camera SpringArm [BETA]

Native `camera.spring_arm(settings)` is **opt-in and script-controlled**: no hidden per-frame updates. It smooths camera follow and rotation, supports movement look-ahead and screen dead zones, and by default retracts around physical obstacles and recovers smoothly after they clear. It is available to gameplay cameras independently of whether the tracked target is a Character.

~~~aeoscript
const boom = camera.spring_arm({
    target: [0, 2, 0],         // tracked world position
    offset: [0, 2, 6],         // desired camera offset (studs)
    dt: 0.016,                // frame delta in seconds
    follow_speed: 8,
    rotation_speed: 12,
    recovery_speed: 10,
    look_ahead: [0, 0, 0],
    dead_zone: [0.1, 0.1],
    collision: true,
    collision_padding: 0.1
})
camera.set_position(boom.position)
camera.set_target(boom.target)
const wall_is_behind_player = boom.obstructed
~~~

**Contract:** Required keys: `target` (three numbers), `offset` (three numbers), `dt` (seconds). Optional: `follow_speed`, `rotation_speed`, `recovery_speed` (0–120); `look_ahead` (three numbers); `dead_zone` (two numbers, each 0–0.95); `collision` (bool, **true** by default); `collision_padding` (0–5 studs); `reset` (bool, clears SpringArm history). The returned map includes `position`, `target` (three-number baskets) and `obstructed` (bool). The engine validates values and only updates when scripts call it. A live gameplay camera and compatible host are required; parser success alone is not a Play-mode validation. Source: `src/scripting/stdlib.rs`, `src/scripting/host/mod.rs`, `src/renderer/camera_spring_arm.rs`.
- **`camera.spring_arm(settings)` [BETA]:** `target` and `offset` ([x,y,z] each) and `dt` (seconds) are required. Optional `follow_speed`, `rotation_speed`, `recovery_speed`, `look_ahead`, `dead_zone`, `collision` (defaults **true**), `collision_padding`, and `reset` control frame-rate-independent following, framing and camera obstruction handling. Returns a map with `position`, `target` and `obstructed`; also positions the current gameplay camera. Target is an ordinary world-space point, independent of Actor/Character handle kind. This native primitive runs only when called by the camera script. Collision currently uses the solid-Cell backend; Actor mesh collision and full live Play verification are **pending**.

---

## 5. UI creation, binding, and typography

~~~aeoscript
const hud = ui.new("Panel")
hud.position = [20, 20]
hud.size = [440, 120]
hud.anchor = "TopLeft"

const title = ui.new("Text")
title.parent = hud
title.text = "Old Flute"
title.font = "Proportional"
title.bold = true
title.italic = false
title.font_size = 22
title.rich_text = true
title.text = "Find the [color=#2980FF]OLD FLUTE[/color]!"
title.horizontal_align = "Left"
title.vertical_align = "Center"

const start = ui.new("Button")
start.text = "Start"
start.on_click = fn() { debug.log("Clicked") }

const quest_text = ui.find("game_hud", "quest_text") // nil if absent
// ui.get(...) has the same arities, but raises if the element is absent.
const viewport = ui.get_viewport_size()               // [width, height]
~~~

**Creatable UI types:** Panel, Text, Button.

**UI operations:** ui.new(type), ui.delete(handle), ui.find(key), ui.find(document_key, key), ui.get(key), ui.get(document_key, key), ui.get_viewport_size().

**Common UI properties:** position [x,y], size [w,h], visible, enabled, color [r,g,b,a], key, anchor, parent (Ui handle/nil), z_index, corner_radius, border_width, border_color [r,g,b,a].

**Text/Button properties:** text, font_size, font ("Proportional" or "Monospace"), bold, italic, rich_text, horizontal_align ("Left"/"Center"/"Right"), vertical_align ("Top"/"Center"/"Bottom"), wrap, padding [left,top,right,bottom], line_spacing. Text additionally has text_scaled (auto-fit text). Button has on_click (function or nil).

**Colored spans:** enable rich_text before using [color=#RRGGBB]text[/color] or [color=#RRGGBBAA]text[/color]. Nonspan text retains normal UI color. Properties must be applied to a valid UI node type; Panel is **not** a Text/Button. Runtime properties are Play-session changes, not writes to the authored UI asset.

---

## 6. Audio emitter and sound

~~~aeoscript
const speaker = cell.new("AudioEmitter")
speaker.position = [0, 1, 0]
speaker.sound.set("audio/ghost_loop.wav")
speaker.sound.looped = true
speaker.sound.MaxDistance = 120
speaker.sound.MinDistance = 10
speaker.sound.volume = 0.7
speaker.sound.pitch(1.189207115)  // Playback rate: pitch and duration change
speaker.sound.play()

speaker.sound.pause()
speaker.sound.play()
speaker.sound.stop()
~~~

**Sound properties:** playing (bool), looped (bool), volume (number), MinDistance/min_distance and MaxDistance/max_distance (number). **Sound methods:** set(path), play(), pause(), stop(), pitch(rate). `pitch(rate)` accepts a finite playback-rate multiplier from **0.25 to 4.0**; 1.0 is the source rate and 2.0 is one octave higher. Speed changes playback duration.

- Both distance-property naming styles are aliases. Base volume remains full up to minimum distance; it falls linearly to zero by maximum distance.
- Default legacy-compatible distances: minimum 10, maximum 100 studs. Runtime clamps values to keep the bounds ordered. Set maximum first when expanding both.
- Listener = gameplay camera. The host resolves imported audio via the active project's audio assets or bundled audio. Runtime Entities can also expose sound handles that follow them.
- Mutating the sound handle affects the active Play session, **not** the authored AudioEmitter settings.
- Every `play()` request can retrigger the sound, including rapid stop/play sequences within one audio update. Pitch is forwarded to the native audio playback path; it is not a persistent project asset or authored emitter property.

---

## 7. Gameplay save data

~~~aeoscript
save.set("coins", 5, 0)
save.set("old_flute", true, false)
save.commit()
const current = save.get("coins")
const starting = save.get_default("coins")
const exists = save.has("coins")

save.reset("coins")            // current <- default; nothing else
save.reset()                   // reset registered values; no Play restart
save.commit()                  // save the reset values if needed
save.remove("temporary_hint")  // explicit field removal, not reset
save.commit()
~~~

**Two values per field:** `save.set(key, value [, default])` stores the current value separately from its persistent default. For a **new field**, omitting the third argument or passing `nil` infers a default by type: `0` (number), `false` (bool), `""` (string). For an **existing field**, two-argument `save.set` preserves the configured default; an explicit third argument replaces it. Current/default types must match.

| API | Effect |
| --- | --- |
| `save.get(key)` / `save.get_default(key)` | Read current value or configured default; missing key -> `nil` |
| `save.set(key, value [, default])` | Stage current value and optionally its default |
| `save.reset(key)` | Restore **one existing** current value to its default |
| `save.reset()` | Restore **all existing** current values to defaults |
| `save.has(key)` | True when the field is registered |
| `save.remove(key)` | Deliberately delete a field; **not** a reset |
| `save.commit()` | Persist staged changes separately |

**Critical: `save.reset()` never restarts/stops Play, creates slots or fields, deletes references, teleports the Player, rebuilds a world, or commits automatically.** New Game position, world/UI refresh and persistence must be authored explicitly in AeoScript. Legacy `save.restart()` is destructive and **not** synonymous with `save.reset()`. Save format v2 stores current and default independently. Version-1 current progress remains intact when loaded; missing defaults are inferred and the file is only migrated on a later write.

---

## 8. Standard math, collections, and strings

### math

| Calls | Meaning |
| --- | --- |
| math.abs(x), math.min(a,b), math.max(a,b) | Absolute/min/max |
| math.floor(x), math.ceil(x), math.round(x) | Rounding |
| math.sqrt(x), math.pow(a,b) | Roots/powers |
| math.sin(x), math.cos(x), math.tan(x) | Trigonometry; angles in radians |
| math.clamp(value,min,max), math.lerp(a,b,t) | Bounds/interpolation |
| math.deg_to_rad(x), math.rad_to_deg(x) | Angle conversion |
| math.random() | Floating-point in [0, 1) |
| math.random(min,max) | **Integer** arguments; inclusive integer range |

~~~aeoscript
const chance = math.random()
const dice = math.random(1, 6)
const angle = math.deg_to_rad(90)
const distance = math.sqrt(9)
const t = math.clamp(0.8, 0, 1)
~~~

### Baskets (zero-indexed)

| Method form | Equivalent namespace form | Result |
| --- | --- | --- |
| items.len() | basket.len(items) | Count |
| items.insert(value) | basket.insert(items,value) | Append |
| items.insert(index,value) | basket.insert(items,index,value) | Insert |
| items.remove() | basket.remove(items) | Remove/return last |
| items.remove(index) | basket.remove(items,index) | Remove/return indexed |
| items.clear() | basket.clear(items) | Clear |
| items.sort() | basket.sort(items) | Sort numbers/strings ascending |
| items.find(value [,start]) | basket.find(items,value [,start]) | Index or nil |
| items.clone() | basket.clone(items) | New outer basket |
| items.freeze() | basket.freeze(items) | Make read-only |
| items.concat([separator [,i [,j]]]) | basket.concat(items,...) | Join as string |
| — | basket.create(count [,value]) | Construct basket |
| — | basket.move(src,a,b,dest_start [,dst]) | Copy/overwrite source range |

~~~aeoscript
const inventory = ["Key", "Old Flute"]
inventory.insert("Potion")
const count = inventory.len()
const index = inventory.find("Old Flute")
const items_copy = inventory.clone()
const display = inventory.concat(", ")
~~~

- basket.create rejects more than **1,000,000** entries and invalid count/index values.
- basket.sort has **no comparator callback**. Mixed incompatible element types produce an error.
- basket.move copies an inclusive range a..b to a destination index; it does **not** erase its source. It can overwrite or extend the destination.
- basket.freeze prevents later mutation. basket.clone is shallow: nested baskets/maps retain their own reference semantics.
- basket.concat takes **0–3 explicit arguments in method form** (separator, start, end).
- Baskets/maps can have cycles; native display/equality code guards against recursive loops.

### Strings and maps

| String method | Namespace form |
| --- | --- |
| text.len() | string.len(text) |
| text.lower() | string.lower(text) |
| text.upper() | string.upper(text) |
| text.reverse() | string.reverse(text) |
| text.split([separator]) | string.split(text [,separator]) |

String length/splitting operate on Unicode scalar values, **not UTF-8 byte length**.

~~~aeoscript
const words = "north,east".split(",")
const data = { item: "Old Flute", count: 1 }
data["count"] = 2
data["unused"] = nil       // remove map entry
const missing = data["absent"] // nil
~~~

There is no general map.* namespace. Index with strings or numbers.

---

## 9. Terrain [BETA]

~~~aeoscript
// Query current effective terrain at horizontal world coordinates.
const surface = terrain.sample_surface(12, 20)

if surface != nil {
    const height = surface.position[1]
    const normal = surface.normal                // [x, y, z]
    const slope = surface.slope_degrees
    const material = surface.material_id        // string or nil
    const origin = surface.source                // "authored" or "runtime"
}

// Session-only generation, using active world terrain settings.
terrain.generate([0, 0, 0], [64, 32, 64])

// Dense runtime smooth-voxel data.
terrain.set_voxels(
    [0, 0, 0],          // origin in studs, integer components
    [2, 1, 2],          // logical sample counts
    ["grass", "rock"],  // material palette
    [
        [0, 1.0],
        [0, 0.65],
        [1, 1.0],
        [1, 0.25]
    ],
    2.0                 // optional spacing_studs
)
~~~

- terrain.sample_surface(x,z) returns map or nil (outside/holes/no terrain). Surface data is a read-only query of the highest effective column, not all cave intersections; slopes/normals are estimates.
- terrain.generate(position,size) needs whole-number 3D coordinates and positive dimensions. It uses the current world's configured generator/settings and writes **runtime** Terrain only.
- terrain.set_voxels(origin,size,materials,voxels[,spacing_studs]) uses a dense array whose length = size.x * size.y * size.z; samples are ordered X-fastest, then Z, then Y. Each sample = [zero-based material index, occupancy 0..1].
- spacing_studs must be finite and positive. Omitting it preserves the historical **4-stud compatibility spacing**; that spacing is not a universal terrain grid rule.
- Runtime generation/set_voxels enforce engine bounds and safety limits. Current documented maximum stud-space volume for these runtime APIs is **16,777,216**; validation can reject unsafe inputs.
- No general terrain.clear(), terrain.set_biome(), or terrain generator-settings mutation API is exposed in this slice.

---

## 10. Particles and runtime effects [BETA]

~~~aeoscript
const created = particles.burst("fire", [2, 3, 4], 24)
const emitter = cell.new("ParticleEmitter")
particles.set_enabled(emitter, true)
const active = particles.count()
particles.clear()
~~~

- particles.burst(preset, [x,y,z] [,count]) validates an installed engine preset and finite coordinates. Default count 24; count must be an integer from **0 to 1024**. Returns the host's created-count result.
- particles.set_enabled(ParticleEmitter Cell, bool) controls a supported emitter. particles.count() returns the current particle count. particles.clear() clears runtime particles.
- Content Browser → Effects also offers built-in **Fire**, **Smoke**, and **Magic** drag sources for authored ParticleEmitters. Dropping one in the viewport places an emitter; dropping it onto an existing **FX Type** target selects that legacy preset. Built-in Fire, Smoke, and Magic share the original unfiltered CPU billboard rendering path in Editor and Play. No new AeoScript API, script syntax, particle renderer or file format is introduced. Project-owned Effekseer FX remain available; live Editor/Play verification of the new drag path is pending.
- Not a scripting API for editing AeoFX emitter curves or authored particle assets.

---

## 11. General-purpose property tweens and Sky Material layers [BETA]

~~~aeoscript
const sky = proj.sky
if sky != nil {
    const material = sky.material
    if material != nil {
        const stars = material.find_layer("Stars")
        if stars != nil {
            stars.alpha = 0.0
            engine.tweens.create({
                target: stars,
                property: "alpha",
                endValue: 1.0,
                time: 5.0,
                easing: "quadOut"
            })
        }
    }
}
~~~

- proj.sky.material is the currently bound Sky Material, or nil. material.find_layer(name) returns a live named layer or nil.
- Read/write layer.alpha between 0 and 1 applies during Play; **does not rewrite .MAT files**.
- engine.tweens.create(options) requires target, property, endValue, time; optional easing = "linear" (default) or "quadOut". Zero duration applies immediately.
- The tween engine is a general-purpose scalar/vector property scheduler with live Actor/Cell/Light/Sound/UI handles and Material-layer references, not a Sky-only binding. Any numeric property already exposed as a real readable/writable native property can use the same API; missing Material controls still need native host bindings before they qualify.
- Real emitted Light color/intensity/range are controlled by `Light.color`, `Light.intensity` and `Light.range`, separate from Cell marker color. `Light.shadows` is boolean and not tweenable. Actor rotation tweening uses quaternion slerp of Euler radians.
- Easing includes linear, quadIn/quadOut/quadInOut, cubicIn/cubicOut/cubicInOut, and sineIn/sineOut/sineInOut.
- Numeric custom attributes can be tweened through existing native storage using a property string such as `"attributes.energy"` on a live Actor, Entity or Cell with an existing numeric `energy` attribute. Direct attribute writes cancel the active attribute tween; bool/string attributes are not interpolated.
- A new tween on the same target/property replaces the prior tween, and direct property assignment cancels it. Bad types/ranges/bindings produce diagnostics. All Play-session tweens are cleared on Play exit.

---

## 12. Diagnostics, test hooks, and compatibility

**Play-mode scene analysis and external telemetry (engine tools, not script APIs):** Development Play sessions record authored Actors (including unspawned objects) and their runtime Entity/Character mappings, movement, scripts, animations and visibility to a bounded `scene-analysis-<unix_ms>` log folder under the project's `.worldkiln/development/logs/`. The latest state and change log show runtime identity, **not** proof an object drew visible pixels. Optional KilnGraph host telemetry is excluded from default builds; instrumented builds require an explicit external collector DLL. Neither feature introduces an AeoScript function. See [W-KILN-CS diagnostics](W-KILN-CS.md#19-diagnostics--development-test-run).

**Authored-world startup [development; runtime unverified]:** Current-format `world.dat` files reject malformed or non-finite Actor position, rotation, scale, mass, and authored collision size/offset. Older-format numeric fallbacks and absent legacy collider tails remain supported. A standalone game now exits with an error instead of running a default scene when the authored world is missing or cannot load. This is a file/runtime compatibility rule, not an AeoScript save-data API; the existing gameplay Save Data is separate.

**Module isolation and Character collision [development 0.8.7; focused release regressions passed]:** Module methods and fields resolve within their owning script. An unset `self.current` remains `nil` instead of discovering another script's `current()` through the combined scene program; legacy non-Module function discovery is retained. No health API or gameplay value is introduced. Character-capable authored Actors retain their identity and skeletal animation, but use authored Box collision rather than visual-asset Auto geometry; authored disable flags and dimensions/transforms are preserved. Default runtime Characters also use boxes. Other Actors retain Auto geometry. See [supported Actor types](../../../reference/Worldkiln-Supported-Actor-Types.md) and [Worldkiln diagnostics](W-KILN-CS.md#19-diagnostics--development-test-run). Live Play/FPS acceptance is pending.

**Runtime diagnostic attribution [development 0.8.7; live capture verified with limits]:** No AeoScript API or gameplay semantics change. Captured runtime errors include script/function/message; failed numeric binary operations record left/right operand types and numeric values when applicable. Existing Detailed capture separates Character movement stages from skeletal pose work, records contact cache/stage counters and bounded expensive Entity-pair visits, attachment parent/child identities, and runtime-to-Actor identity inventories. Saved `diagnostic_events.csv` and optional `capture.json` diagnostic facts retain frame/time and known/unknown self time; inclusive durations must not be added across parents and children. A release capture attributes the dominant recorded Character/contact cost to four skeletal Actors; sample eviction and unmeasured total instrumentation overhead limit interpretation. Post-fix live FPS remains unverified. See [Worldkiln diagnostics](W-KILN-CS.md#19-diagnostics--development-test-run) for capture limits and build provenance.

**Physics diagnostic counter clarification [source-edited; runtime unverified]:** Existing detailed contact stage counts distinguish broadphase candidate pairs from entity/Character narrowphase attempt counts. The structured diagnostic also reports confirmed_entity_contacts and confirmed_character_contacts only when native contact detection succeeds; cached contacts and other support paths are not included in those two hit counts. This is diagnostic data, not an AeoScript API. Native broadphase add/remove counters are not yet implemented.

**Engine-owned script performance diagnostics [development 0.9.5]:** The bottom **Diagnostics** dock and **Settings → Development → Test Run** share one recorder. AeoScript fibers are profiled by script path and individual execution time while recording; user-configurable slow script slices are stored in `script_spikes.csv` (default 20 ms threshold). Time spent in native `find_path()` and `navigate_to()` and their navmesh build/refresh/path-query internals is attributed to the invoking script in `navigation_calls.csv`. The live view ranks expensive script/native/engine spans and lists recent stalls. This introduces **no script API, event, or automatic game behavior**; the trace runs only while explicitly recording. Timings are overlapping wall-clock durations, not a call-stack sampling profiler or automatic proof of root cause.

**Diagnostics workspace compatibility [development 0.9.5; release compilation and lint checks passed; live acceptance pending]:** Existing scripts and navigation methods are unchanged. Editor **F5** toggles Play, **F2** hides/restores the bottom tray during Play, and **Escape** remains gameplay input. Bindings are configurable in Input settings; standalone input is unchanged. Settings and Diagnostics share a session. Pause freezes presentation while collection continues; Start resumes the display. Versioned `capture.json` preserves frame/thread/span/parent identity, measured inclusive/self durations, asynchronous request links for native navigation jobs, callback source byte offsets and script/instance/native target identity where captured; unknown optional fields are tolerated and unsupported format versions fail explicitly. Self time subtracts captured child spans only. Standard and Detailed capture are explicit options independent of open views. Existing CSV/summary reports remain compatible; older logs cannot recover missing nesting or timings. Saved capture comparison reports mismatched conditions without claiming causation. Missing GPU timing, uninstrumented work and evicted samples remain disclosed. See [Diagnostics workspace and measurement limits](W-KILN-CS.md#19-diagnostics--development-test-run).




~~~aeoscript
print("Ready", time.delta)
debug.log("Position", player.position)

test.complete("chest_open", true)
const done = test.is_completed("chest_open")
const passed = test.passed("chest_open")  // may be nil if pending
const totals = test.summary()            // [passed, failed, total]
~~~

**Compatibility globals** (prefer the namespaced equivalent where available): enable_script(path), disable_script(path), is_script_enabled(path), fire_event(name,...), complete_test(name,passed), is_test_completed(name), test_passed(name), test_summary(), get_entity(name), get_light(x,y,z), find(name).

**Important compatibility differences from the 0.7.x sheet:**

| Legacy idea | V1-target / current source truth |
| --- | --- |
| x = 10 creates a new variable | Declare with const x = 10 or x: number = 10 first |
| Only Cell/Entity object methods | Distinct Actor handles, physics/animation capabilities, navigation |
| Every script update(dt) automatically runs | Automatic per-frame Entity update is specific to legacy Cell-bound Entity scripts |
| Global find as the primary Actor lookup | Prefer proj.find / proj.find.all and folder-scoped lookup |
| Runtime changes persist automatically | Runtime changes reset when Play ends; use save.* for supported persistent primitives |
| cell.new("NPC") creates a cell | NPC is **not** a supported runtime cell.new type |
| UI has only text and position | Script UI supports typography, layout, named lookup and rich color spans |
| Sound has only play/stop/volume | set(path), pause, looped, min/max attenuation distances |
| Terrain is always a 4-stud voxel grid | Runtime set_voxels has explicit optional spacing; 4 is compatibility default |
| Arbitrary time interpolation via engine.tweens | Only registered property targets (currently layer.alpha) are script-exposed |

### Error patterns to avoid

~~~aeoscript
// WRONG: assigning to an undeclared variable.
// coins = 10

// RIGHT:
coins: number = 10
coins += 1

// WRONG: number is not a boolean condition.
// if coins { }

// RIGHT:
if coins > 0 {
    debug.log("Has coins")
}

// WRONG: Entity/Actor.set_position requires 3 scalar arguments.
// guard.set_position([2, 0, 2])

// RIGHT (when guard is a valid Actor/Entity handle):
// guard.set_position(2, 0, 2)

// WRONG: script values cannot serialize baskets as save data.
// save.set("inventory", ["Old Flute"])

// RIGHT:
save.set("has_old_flute", true)
save.commit()
~~~

### Verification / source-of-truth pointers

**Build Game integration [release-compiled; exported Play unverified]:** AeoScript runtime semantics, project scripts and Actor Modules are unchanged by the export backend correction. The existing Windows game package retains manifest format 1, now permitting optional versioned export metadata while preserving legacy package parsing. Its payload still contains the saved authored project, and the runtime may download separately verified engine resources on first launch. Do not describe source-only export fixes as successful standalone execution, Linux support, offline launch or a new scripting API.

| What to verify | Authoritative implementation |
| --- | --- |
| Lexing, declarations, expressions, errors | src/scripting/lexer.rs, parser/*, semantic.rs |
| Runtime value types, truthiness, handle kinds | src/scripting/value.rs |
| Namespaces, builtin arities, math/basket/save/terrain/tags/observe | src/scripting/stdlib.rs |
| Global functions, host contract | src/scripting/api.rs |
| Actor/Entity/Cell/Ui/Sound property and method bridge | src/scripting/host/mod.rs, host/*, scene.rs |
| Module resolution, project lookup, event dispatch | src/scripting/interpreter/*, runtime.rs, scene.rs |
| Particle API | src/scripting/particle_api.rs |
| Tween property binding | src/scripting/tween.rs, stdlib.rs and implementing engine host |
| Broader examples and narrative behavior | docs/language/AeoScript_*.md (secondary to source) |

**Scope:** This document records identified script-visible contracts in the current development working tree, not a guarantee that every host implements every operation or that the V1 public release is complete. Update this cheat sheet when implementation changes; do not silently copy old legacy examples into V1.

---

---

---

---

---

**Game export compatibility:** Windows Shipping supports folder/ZIP delivery and output-parent selection through the packaging backend. AeoScript content is preserved. Development runtime and new editor controls are pending.

---

**Packaging editor compatibility (left-docked workspace):** Build Game opens the docked Packaging workspace. Windows Shipping supports editable, per-project saved title, game version, executable name, icon (ICO/PNG), startup window mode and resolution, alongside folder/ZIP export, build cancellation and Build & Run. The export manifest carries the identity settings; the game runner applies the window settings. AeoScript APIs and authored scripts are unchanged. Source compilation is verified, while icon, executable launch and clean-profile runtime acceptance remain unverified.

---
