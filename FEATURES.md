# Worldkiln features

[Download](https://github.com/Aeowun/Worldkiln/releases) · [User guide](DIVE.md) · [Documentation](https://aeowun.com/docs/)

Worldkiln combines a 3D world editor, AeoScript gameplay, and standalone Windows game builds.

## Worlds and environments

**Cells** are Worldkiln's building pieces for structures, platforms, interiors, and detailed objects. Place, erase, select, transform, and edit them directly in the viewport.

**Landscaping** provides terrain generation, sculpting, smoothing, flattening, and material painting. Organize generated landscape layers and adjust the environment around them.

**Water** combines animated waves with collision-driven ripples, wakes, foam, and underwater camera effects. Set up oceans, lakes, and rivers with their own water settings.

**Materials and lighting** control the look of Cells, Actors, landscapes, skies, and water. Distance fog and particle effects add atmosphere.

## Actors and assets

Import **GLB models** and place them as Actors. Arrange and transform props, configure collisions, and use animated Character packages.

The **Asset Manager** and **Content Browser** organize models, textures, audio, media, AeoScript files, and other project assets. Drag supported assets into the scene or select them for editing.

### Asset Manager and Content Browser

Browse **Actors, Characters, Controllers, Cameras, Textures, Materials, Images, Video, Audio, Effects, and Scripts** in consistent project categories.

- **Import assets:** Select external GLB models, images, audio, video, and effects, or install assets from AssetPacks.
- **Create Characters:** Import a rigged or animated GLB as an Actor, then create a Character with its own scale, material, Controller, and Camera.
- **Explore models:** Preview Actors and Characters, inspect meshes and animations, and view material and texture references.
- **Organize assets:** Give assets custom names and groups without changing their type or identity.
- **Manage resources:** Review imports, replace assets, and see dependencies before deleting resources.
- **Work across the editor:** Select and inspect the same project assets in Asset Manager and Content Browser, and drag Actors into the scene.

## Gameplay

**AeoScript** connects gameplay to the World: object queries, input, events, movement, camera control, UI, audio, animation, and game state. Scripts can be reused as Modules or attached to game objects.

Worldkiln provides **physics and navigation** for moving Characters and Actors, collision events, path queries, and navigation-driven behavior.

Create **game UI** with Panels, Text, and Buttons. Add audio emitters and particle effects to scenes, and use AeoScript to control their runtime behavior.

Game progress can be stored through AeoScript's `save` APIs, separately from the scene saved in the editor.

## From editor to game

Use **Play mode** to test movement, collisions, scripts, cameras, and interfaces. **Build Game** creates a standalone Windows game from the project.

For instructions, see the [user guide](DIVE.md). For scripting details, see [AeoScript](AEOSCRIPT.md) and the [API reference](https://aeowun.com/docs/reference/).

