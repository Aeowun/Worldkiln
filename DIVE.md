# Worldkiln user guide

This guide covers the path from a new project to a playable Windows game. For a list of tools, see [Features](FEATURES.md); for scripting, see [AeoScript](AEOSCRIPT.md).

## Install and create a project

Download Worldkiln from [GitHub Releases](https://github.com/Aeowun/Worldkiln/releases). The editor runs on Windows 10 and 11.

Open Worldkiln, create a project from Home, and open it in the editor. Existing projects can be reopened from Home.

![Worldkiln editor workspace](SCREENSHOTS/02-editor-overview.png)

## Build a scene

Use the **Build** tools to place and edit **Cells** for floors, walls, platforms, and structures. Select a Cell to change its properties. Use Undo and Redo to revise an edit.

For an outdoor scene, open **Terrain** to generate land and **Landscaping** to sculpt or paint it. Adjust waves, water bodies, materials, lighting, sky, and fog to establish the environment.

Use the **Project Explorer** to organize scene objects and scripts. The **Properties** panel edits the current selection; the **Content Browser** provides assets for the scene.

## Add assets and characters

Open the **Asset Manager** to import project files or browse asset packs. Worldkiln accepts GLB models for Actors, along with project textures, audio, and other supported media.

Drag an Actor into the scene, then move, rotate, and scale it. Configure collision and other Actor properties before testing in Play mode.

Choose a compatible Character package, then configure its animations, Controller, and Camera in **Project Settings**.

## Add gameplay

Open the **Script Editor** and create a `.aeo` project script. AeoScript handles input, events, Actors, Cells, UI, sound, cameras, and gameplay state.

Use the [AeoScript guide](AEOSCRIPT.md) for a working event example. The [API reference](https://aeowun.com/docs/reference/) lists exact signatures and return values.

For on-screen interfaces, open the **UI Editor**. Create Panels, Text, and Buttons, assign script keys to interactive elements, and connect their behavior to AeoScript.

![UI authoring in Worldkiln](SCREENSHOTS/39-ui-editor-workspace.png)

## Test in Play mode

Press **Play** to run the scene. Test Character movement, collisions, interactions, UI, animation, and sound. Open **Output** or **Problems** to inspect scripting and runtime errors.

Stopping Play returns to the authored scene. Changes made only during Play do not replace the saved scene. Persistent game progress uses AeoScript's `save` API.

## Build and share

Select **Build Game** to package the project as a standalone Windows game. Launch the built game outside the editor and verify its startup, assets, controls, and saved-game behavior before distributing it.

## Help

[Getting started](https://aeowun.com/worldkiln/getting-started/) · [Documentation](https://aeowun.com/docs/) · [AeoScript](AEOSCRIPT.md) · [Support](https://aeowun.com/support/) · [Changelog](CHANGELOG.md)

See the [license](LICENSE) for terms of use.
