# Worldkiln

> # **Make a game. Make it yours.**
>
> **Worldkiln 0.8.5 is the current public beta.**

![Worldkiln — Make a game. Make it yours.](SCREENSHOTS/Worldkiln_%20Build%20Your%20Fantasy%20Fortress.png)

Worldkiln is a game engine and development environment for making your own games.

Build your world visually. Add Actors and Characters. Create gameplay with AeoScript. Design UI. Press Play and test directly in the editor.

When you're ready, package your project as a standalone game.

**BUILD → SCRIPT → PLAY → SHIP**

[**Download Worldkiln for Windows →**](https://github.com/Aeowun/Worldkiln/releases)

[Documentation](https://aeowun.com/docs/) · [Getting Started](https://aeowun.com/worldkiln/getting-started/) · [AeoScript](https://aeowun.com/aeoscript/) · [DIVE](DIVE.md)

---

## Build Your Project

Worldkiln keeps your projects, Worlds, Actors, scripts, UI, assets, and builds together in one workspace.

![Worldkiln project manager](https://github.com/user-attachments/assets/ed277a84-1529-4e7e-bbaa-e402e1ea5e51)

Create a project, build your World, add gameplay systems, test them, and keep iterating.

---

## Build Worlds. Add Actors.

Worldkiln is built around visual World authoring.

Place and edit voxel Blocks, import project assets, add Characters and Actors, and configure their transforms, physics, packages, Attributes, and Modules.

![Editing an Actor transform in Worldkiln](https://github.com/user-attachments/assets/45c6fe55-d19d-4e68-b195-8fc2bacf4675)

Actors can become Characters, NPCs, props, tools, items, doors, switches, or whatever your game needs.

---

## Script With AeoScript

AeoScript is Worldkiln's gameplay scripting language.

Scripts can control Actors, input, events, UI, audio, animation, cameras, collisions, navigation, and game state.

Write `.aeo` files directly inside Worldkiln.

![AeoScript editor with linked API documentation](https://github.com/user-attachments/assets/cfdc7f54-df42-4900-9d75-07cbf8554b1c)

The Script Editor can link API symbols directly to the Worldkiln documentation while you work.

```aeoscript
const actor = self.parent
const speed = 2.0

// Use the Actor's authored Y rotation.
const yaw = actor.rotation[1]

// Convert rotation into a forward X/Z direction.
const dir_x = math.sin(yaw)
const dir_z = math.cos(yaw)

// Face and move forward.
actor.set_facing_direction(dir_x, dir_z)
actor.set_horizontal_velocity(
    dir_x * speed,
    dir_z * speed
)

// Play the walking animation.
actor.select_animation("Walk")
```

[**Explore AeoScript →**](https://aeowun.com/aeoscript/)

---

## Play. Test. Iterate.

Press Play and run your game directly inside the editor.

Worlds, Actors, physics, scripts, UI, audio, animation, cameras, and input come together in one running project.

Change something. Run it again.

Then package the project as a standalone game when you're ready.

**Worldkiln is the tool. Your game is the product.**

---

## Still Early

Worldkiln is under active development and has not reached version 1.0.

The current public beta targets Windows and may contain bugs, unfinished features, compatibility issues, and breaking changes.

For the deeper technical breakdown, feature coverage, and architecture:

[**Read DIVE.md →**](DIVE.md)

For release history and development work:

[**Read CHANGELOG.md →**](CHANGELOG.md)

---

## Build With Us

Worldkiln is still being shaped by the people using it before 1.0.

Report bugs. Build projects. Test systems. Tell us what is missing.

[**Join AEOWUN →**](https://aeowun.com/members/signup/) · [**Founding Developer Program →**](https://aeowun.com/worldkiln/founders/)

---

## Support Worldkiln

Worldkiln is independently developed.

[**Support Worldkiln on Ko-fi →**](https://ko-fi.com/aeowun/tip)

---

*Worldkiln is under active development. Features, APIs, editor behavior, project formats, and gameplay behavior may change between beta releases.*
