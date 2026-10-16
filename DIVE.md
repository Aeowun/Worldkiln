# Worldkiln User Guide

**Make a game. Make it yours.**

Worldkiln lets you build environments, place characters and objects, create gameplay, and turn your work into a standalone Windows game. You can work visually in the editor and use **AeoScript** when you want to make things move, react, and behave.

[Download Worldkiln](https://github.com/Aeowun/Worldkiln/releases) · [Explore Features](FEATURES.md) · [Getting Started](https://aeowun.com/worldkiln/getting-started/) · [AeoScript](AEOSCRIPT.md) · [Documentation](https://aeowun.com/docs/)

> **About BETA:** A BETA feature is **included in Worldkiln and available to use**. It may still have rough edges, bugs, or changes to how it works. BETA is a maturity label—not a promise that a feature will arrive in a later download.

## Getting started

Worldkiln supports Windows 10 and 11.

1. Download and install Worldkiln from the [Releases page](https://github.com/Aeowun/Worldkiln/releases).
2. Launch Worldkiln and create a project or open an existing one.
3. Start with a simple scene. Build with **Cells**, create a **Terrain**, or import an **Actor**.
4. Add a Character, a Spawn Point, or an interaction.
5. Press **Play** to try the game. Stop Play mode to return to editing.
6. Use **Build Game** when you want a standalone Windows game.

By default, Worldkiln keeps its installed files and project data under the Aeowun application directory, including `%LOCALAPPDATA%\Aeowun\UserData\`. You can manage project files in Windows File Explorer. Keep backups of projects you care about.

## Inside the editor

The workspace brings your scene and the tools for editing it together.

- **Viewport:** look around the World, select objects, and work directly on your scene.
- **Project Explorer:** organize the project, Actors, scripts, and scene objects in folders.
- **Properties:** edit the settings for the selected object.
- **Build:** create and edit Cells and other world objects.
- **Terrain and Landscaping:** generate, sculpt, paint, and shape outdoor environments.
- **Materials:** create and edit the appearance of surfaces, water, and skies.
- **Content Browser and Asset Manager:** find assets, import files and packs, and use drag-and-drop where an asset is needed.
- **Script and UI workspaces:** create AeoScript gameplay and in-game interfaces.
- **Output and Problems:** see relevant messages when something in your project needs attention.

You can customize the editor's theme, accents, typography, and layout preferences in Settings. Project-specific controls belong to **Project Settings**.

## Building with Cells

**Cells** are Worldkiln's precise building pieces. Use them to create floors, walls, structures, platforms, and detailed world objects.

You can place and erase Cells, build along lines or planes, select multiple Cells, copy and paste, and adjust properties such as color and solidity. Cells support materials and project-owned textures where appropriate. Editor Undo and Redo let you revise your work.

Cells and terrain are different tools: use Cells for placed structures and **Landscaping** for larger outdoor landforms.

## Terrain and Landscaping

Worldkiln's Terrain tools let you create an environment from a generated landscape, then shape it in the editor.

- Generate terrain and choose the landform's settings.
- Sculpt with tools such as Raise, Lower, Smooth, and Flatten.
- Paint terrain materials and work with shoreline and slope transitions.
- Use grass, sand, rock, and other layers to establish different ground surfaces.
- Adjust water level and landscape appearance as you build your scene.

Some landscaping controls are labeled **BETA**. They are available to use, but large or complex scenes may still need extra care.

## Water, shorelines, and atmosphere

Water in Worldkiln does more than sit beside the land.

- Waves can be configured to change the feel of the ocean.
- Objects interacting with water can produce ripples, wakes, and foam.
- Shoreline effects bring water visually into contact with the beach.
- Wetness and drying effects help recently washed surfaces look different from dry ground.
- Underwater camera effects change what you see beneath the animated surface.

Use Water, Sky, and Surface Materials to shape the scene's colors and appearance. Lighting, sky settings, and distance fog help create the atmosphere around your world. Water and some material controls are **BETA** features.

## Materials and visual effects

The Material Editor lets you create reusable looks for your scene. Depending on the material type, you can work with surface properties, layered materials, skies, and water appearance.

You can also add particle effects to your World—for example, fire or smoke—and control supported emitters through AeoScript. Not every effect type is meant to replace a dedicated simulation; choose tools based on the look and behavior your game needs.

## Assets and Actors

An **Actor** is an object you add to your World: a character, prop, interactive item, or another part of the game. Imported 3D models, including supported GLB packages, can be used as Actors.

Through the editor you can:

- Import project assets and browse available asset packs.
- Place or duplicate Actors, then move, rotate, and scale them.
- Assign compatible materials and other project assets using supported drag-and-drop fields.
- Set up Character and Camera Controllers in Project Settings.
- Choose a Character package and work with its available animations.
- Configure Actor properties, script bindings, and collision behavior.

The Asset Manager organizes project-owned files. Your source assets and project data remain part of your project; keep backups before replacing or deleting important files.

## AeoScript: make the World interactive

**AeoScript** is Worldkiln's built-in gameplay language. It uses `.aeo` files to connect the World to player input, object behavior, cameras, animation, sound, UI, events, and gameplay systems.

```aeoscript
debug.log("Game started")

gold: number = 0

on on_gold(amount) {
    gold += amount
    debug.log(gold)
}
```

You can write standalone scripts, attach scripts to supported game objects, and reuse Modules. AeoScript includes values and collections, functions, events, closures, and `wait()` for gameplay that happens over time.

Script-driven objects can respond to contact events, move through supported navigation paths, play sounds and animations, and update the user interface. The Script Editor includes errors and other tools to help you find problems.

See the [AeoScript Language Overview](AEOSCRIPT.md) for examples and a closer look at the language.

## UI for your game

The UI workspace lets you design what players see on screen, including Panels, Text, and Buttons.

You can position and style interface elements, work with anchors and layout guides, and connect supported UI elements to AeoScript. The editor includes snapping, preview sizes, and other tools for arranging game interfaces.

AeoScript can also create and update UI at runtime. Use the UI workspace for authored layouts and scripts for game-driven changes.

![Worldkiln UI workspace](SCREENSHOTS/39-ui-editor-workspace.png)

## Characters, animation, and movement

Worldkiln supports Characters with movement, physics, animation, and gameplay cameras. Compatible Character packages can include multiple animation clips.

Use AeoScript to control supported player movement, camera behavior, animation actions, or object attachments. Objects can attach to named points on a Character rig and detach during gameplay.

Characters and Actors can interact with colliders and solid World geometry in Play mode. Physics behavior is distinct from the scene you have authored and should be tested as you build.

## Navigation

Worldkiln includes navigation tools for helping Actors move through suitable parts of a World. You can configure navigation agent dimensions and traversal limits, inspect navigation data, request paths, and respond to navigation completion or failure from AeoScript.

Navigation depends on the surfaces and obstacles in your scene. Check the navigation view again after changing the World.

## Play mode and saved games

**Play mode** is where you try what you have created. Test movement, physics, scripts, audio, UI, cameras, and object interactions without replacing the authored World with the temporary simulation.

Ordinary Play-mode changes reset when you stop. If your game needs to remember progress, AeoScript provides persistent save values.

```aeoscript
save.set("gold", 250)
save.set("quest_complete", true)
save.commit()
```

Read a value with `save.get("gold")`. Use `save.commit()` to store changes. Saved game progress is kept separately from the authored World.

## Build your game

When you are ready to share your project, choose **Build Game** to package a standalone Windows executable. The game runs independently of the Worldkiln editor and uses the matching runtime assets it needs.

Check your game outside the editor before sharing it, particularly its asset use, first launch, saves, and any BETA features your game depends on.

## Getting help

- [Worldkiln documentation](https://aeowun.com/docs/)
- [Getting started](https://aeowun.com/worldkiln/getting-started/)
- [AeoScript guide](AEOSCRIPT.md)
- [Changelog](CHANGELOG.md)
- [Support](https://aeowun.com/support/)
- [Founding Developer Program](FOUNDING_DEVELOPERS.md)

## License

Worldkiln is source-available software and is **not** distributed under an open-source license. Read the [LICENSE](LICENSE) for the complete terms.

---

**Worldkiln is the tool. Your game is the product.**
