<img width="2401" height="1244" alt="actor-transform" src="https://github.com/user-attachments/assets/45c6fe55-d19d-4e68-b195-8fc2bacf4675" />
<img width="1913" height="1079" alt="actor-navigation-script" src="https://github.com/user-attachments/assets/cfdc7f54-df42-4900-9d75-07cbf8554b1c" />
<img width="1914" height="1079" alt="worldkiln-home" src="https://github.com/user-attachments/assets/ed277a84-1529-4e7e-bbaa-e402e1ea5e51" />
<img width="895" height="448" alt="navigation-aeoscript" src="https://github.com/user-attachments/assets/268af317-3bdb-4e53-a0af-fdff643b5022" />
# Worldkiln

> # **Make a game. Make it yours.**
>
> **Worldkiln 0.8.0 is the current public beta.**

<p align="center">
  <img src="SCREENSHOTS/Worldkiln_%20Build%20Your%20Fantasy%20Fortress.png" alt="Worldkiln 0.8.0 — Make a game. Make it yours." width="100%">
</p>

Worldkiln is a game engine and development environment for making your own games.

Build your world visually. Add Actors and Characters. Create gameplay with AeoScript. Design UI. Press Play and test your game directly in the editor.

When you're ready, package your project as a standalone game.

**BUILD → SCRIPT → PLAY → SHIP**

[**Download Worldkiln 0.8.0 for Windows →**](https://github.com/Aeowun/Worldkiln/releases/download/win11-v0.8.0/Installer.exe)

[Documentation](https://aeowun.com/docs/) · [Getting Started](https://aeowun.com/worldkiln/getting-started/) · [AeoScript](https://aeowun.com/aeoscript/)

---

## Make the World

Worldkiln gives you a visual workspace for building the environment your game takes place in.

The current World editor is built around voxel construction.

Place Blocks. Shape spaces. Paint surfaces. Select, move, copy, and organize parts of your World.

Import textures and other project assets directly into your project.

<p align="center">
  <img src="SCREENSHOTS/screenshoot_castle.png" alt="A castle and settlement built in Worldkiln" width="100%">
</p>

<table>
<tr>
<td width="50%">
<img src="SCREENSHOTS/v%200.8.x%20editor.png" alt="Building a World in the Worldkiln editor">
</td>
<td width="50%">
<img src="SCREENSHOTS/Untitled.png" alt="A Worldkiln landscape with a hilltop structure">
</td>
</tr>
<tr>
<td align="center"><strong>BUILD IT</strong></td>
<td align="center"><strong>MAKE IT YOURS</strong></td>
</tr>
</table>

World building is one part of the engine—not the definition of what your game has to be.

---

## Add the Things That Make It a Game

Actors are objects you place in your project and give purpose through their properties, physics, assets, and scripts.

An Actor might be:

- A Character or NPC.
- A prop.
- A weapon.
- A tool.
- An item.
- A switch.
- A door.
- An interactive object.
- Something specific to your game.

<p align="center">
  <img src="SCREENSHOTS/v%200.8.x%20actor.png" alt="Worldkiln Actor editing" width="100%">
</p>

Actors have their own transforms, packages, physics settings, Attributes, and AeoScript Modules.

The Project Explorer keeps Worlds, Actors, scripts, UI, and project content together as the game grows.

<p align="center">
  <img src="SCREENSHOTS/v%200.8.x%20explorer.png" alt="Worldkiln Project Explorer" width="100%">
</p>

---

## Make It Do Something

### AeoScript

AeoScript is Worldkiln's gameplay scripting language.

It is designed around the things you actually need to describe when making a game: objects, input, events, UI, audio, animation, cameras, collisions, game state, and reusable gameplay systems.

Write `.aeo` files directly inside Worldkiln.

<p align="center">
  <img src="SCREENSHOTS/blacksmith_patrol.png" alt="AeoScript controlling a blacksmith patrol with navigation callbacks" width="100%">
</p>

A simple script can start small:

```aeoscript
debug.log("Game script loaded")

gold: number = 0

on on_gold(amount) {
    gold += amount
}
```

AeoScript can control:

- Actors and Entities.
- Input.
- Events.
- UI.
- Audio.
- Animation.
- Attachments.
- Player behavior.
- Cameras.
- Collision events.
- Game state.
- Reusable Modules.

Scripts can belong to individual game objects or coordinate systems across the entire game.

You are still programming your game.

Worldkiln's job is to make that programming approachable and keep it close to the game you're building.

[**Explore AeoScript →**](https://aeowun.com/aeoscript/)

---

## Press Play

You shouldn't need to leave your project just to find out whether something works.

Play mode runs the game directly inside Worldkiln.

Your World, Actors, physics, scripts, UI, audio, animation, cameras, and input come together as the game you're creating.

Worldkiln keeps script output available inside the editor while the game runs.

<p align="center">
  <img src="SCREENSHOTS/moon.png" alt="A game running in Worldkiln Play mode with a character, HUD, and dialogue" width="100%">
</p>

Stop Play mode, make another change, and run it again.

Ordinary gameplay changes made during Play mode do not overwrite the project you're building.

---

# Ship Your Game

Worldkiln isn't where your finished game has to live.

Worldkiln can package a project as a standalone game that runs independently from the editor.

```text
Create a project
        ↓
Build your world
        ↓
Add Actors and Characters
        ↓
Write gameplay
        ↓
Create UI
        ↓
Press Play
        ↓
Build game
        ↓
Your game
```

Your players don't need to create a Worldkiln project to play what you made.

They don't need the Worldkiln editor to run the standalone game.

**Worldkiln is the tool. Your game is the product.**

---

## One Workspace

Worldkiln brings the major parts of making a game into the same development environment.

### World Building

Build and edit the environment visually.

### Actors & Characters

Create the objects, Characters, NPCs, props, items, and interactive parts of the game.

### AeoScript

Write the rules and behavior that turn those pieces into gameplay.

### UI

Create interfaces and connect them to the game through AeoScript.

### Physics

Use collision, gravity, moving objects, Characters, and contact events.

### Animation

Use Character animation, layered actions, and attachment points.

### Audio

Import WAV assets, place Audio Emitters, and control sound through AeoScript.

### Play Mode

Run and debug the game without leaving the editor.

### Standalone Builds

Package the project into a game that runs separately from Worldkiln.

---

## From Project to Game

Worldkiln is being built around a straightforward development loop:

```text
BUILD
  ↓
SCRIPT
  ↓
PLAY
  ↓
ITERATE
  ↓
SHIP
```

Start with something small.

Make it move.

Give it rules.

Break it.

Fix it.

Add another system.

Keep building until it becomes your game.

---

## Worldkiln Is Still Early

Worldkiln is under active development and has not reached version 1.0.

The current public release is:

**Worldkiln 0.8.0 Beta**

Worldkiln currently targets **Windows 10 and Windows 11**.

Public beta builds may contain bugs, unfinished features, compatibility issues, and breaking changes.

The engine is growing quickly, and its current capabilities do not define the limits of what Worldkiln is intended to become.

---

## Try Worldkiln

You can install the current Windows build and start a project today.

[**Download Worldkiln 0.8.0 →**](https://github.com/Aeowun/Worldkiln/releases/download/win11-v0.8.0/Installer.exe)

You can also find current and previous builds on the [Worldkiln Releases](https://github.com/Aeowun/Worldkiln/releases) page.

[**Getting Started →**](https://aeowun.com/worldkiln/getting-started/)

---

## Documentation

Learn the engine and its scripting language:

[**Worldkiln Documentation →**](https://aeowun.com/docs/)

[**Getting Started →**](https://aeowun.com/worldkiln/getting-started/)

[**AeoScript →**](https://aeowun.com/aeoscript/)

For a deeper technical overview of Worldkiln's current systems, see [DIVE.md](DIVE.md).

For release history and current development work, see [CHANGELOG.md](CHANGELOG.md).

---

## It Started Smaller

Worldkiln began as AeoEngine.

### The First World

<p align="center">
  <img src="SCREENSHOTS/first_world.png" alt="The first Worldkiln world" width="80%">
</p>

### Early AeoScript

<p align="center">
  <img src="SCREENSHOTS/first_aeoscript.png" alt="Early AeoScript" width="100%">
</p>

### AeoEngine 0.7.x

<p align="center">
  <img src="SCREENSHOTS/v%200.7.x_menu.png" alt="AeoEngine 0.7.x" width="80%">
</p>

What started as a small world editor has been growing into a complete environment for making games.

There is still a long way to go.

**Make a game. Make it yours.**

---

## Founding Developers

Worldkiln is still being shaped by the people building with it before 1.0.

The **Worldkiln Founding Developer Program** recognizes members who meaningfully build with, test, and help improve Worldkiln during its founding era.

AEOWUN Members is the community and contribution layer behind that program. Members can maintain a profile, share projects and updates, publish games, submit builds, report bugs, give feedback, and participate in the community. Useful participation can be recorded as contribution history, and verified contributions can place a member into the Founding Developer review queue.

The program is intentionally not automatic. Creating an AEOWUN member account does **not** grant Founding Developer status. A member becomes a candidate through verified contribution, and Founding Developer status is awarded after review for meaningful participation during Worldkiln's pre-1.0 era.

The relationship is:

```text
AEOWUN Member
      ↓
Builds, tests, reports, documents, gives useful feedback, or contributes
      ↓
Contribution recorded
      ↓
Contribution verified
      ↓
Founding Developer candidate
      ↓
AEOWUN review
      ↓
Founding Developer
```

This keeps the distinction clear:

- **Member** — anyone with an AEOWUN account.
- **Contributor** — a member with recorded or verified useful participation.
- **Founding Developer** — a contributor explicitly recognized by AEOWUN for meaningful Worldkiln participation during the pre-1.0 founding era.

Founding Developer status is intended to be permanent. Program benefits and opportunities may evolve as Worldkiln grows.

[**Join AEOWUN Members →**](https://aeowun.com/members/signup/)

[**Learn about the Founding Developer Program →**](https://aeowun.com/worldkiln/founders/)

[Program charter](FOUNDING_DEVELOPERS.md)

---

## Support Worldkiln

Worldkiln is independently developed.

If you want to support continued development:

[**Support Worldkiln on Ko-fi →**](https://ko-fi.com/aeowun/tip)

---

<sub>Worldkiln is under active development. Features, APIs, editor behavior, project formats, and gameplay behavior may change between beta releases.</sub>
