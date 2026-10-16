# Worldkiln features

Worldkiln is a 3D game engine and visual editor for Windows. Its tools cover world construction, terrain, water, materials, characters, gameplay scripting, and standalone game builds.

[Download Worldkiln](https://github.com/Aeowun/Worldkiln/releases) · [View examples](SHOWCASE.md) · [Getting started](https://aeowun.com/worldkiln/getting-started/) · [AeoScript reference](https://aeowun.com/docs/reference/)

![A fortress scene built in Worldkiln](SCREENSHOTS/Worldkiln_%20Build%20Your%20Fantasy%20Fortress.png)

**Explore:** [World building](#world-building) · [Water](#water-and-shorelines) · [Materials](#materials-lighting-and-atmosphere) · [Characters](#actors-animation-and-physics) · [Scripting](#aeoscript-and-gameplay) · [UI and media](#interfaces-audio-and-media) · [Effects](#particles-and-effects) · [Editor](#the-editor-and-asset-library) · [Publishing](#play-testing-and-publishing)

## World building

### Cells and construction
- Place, erase, select, copy, and paste Cells to build structures, interiors, and platforms.
- Use line and plane building tools, placement previews, and individual Cell properties.
- Control appearance and solidity without having to create a separate imported mesh for every piece.

### Terrain and landscaping · BETA
- Generate terrain with adjustable seeds, height, variation, and water level.
- Sculpt with Raise, Lower, Smooth, and Flatten brushes, including brush falloff and painted surface layers.
- Build landscapes with separate generation layers, keeping the world editable after its initial shape is created.
- Paint grass, rock, sand, and custom terrain material layers; use slope and shoreline relationships to blend ground surfaces.
- Add procedural grass and edit waterline regions alongside the terrain.

### Scene organization and group editing
- Arrange Actors, scripts, and scene objects in Project Explorer folders.
- Select multiple Actors in the Explorer or viewport; move, rotate, and scale them together.
- Edit shared Actor properties, with mixed-value indicators when selected objects differ.
- Duplicate groups while preserving their relative placement. Multi-Actor editing is **BETA**.

## Water and shorelines · BETA

### Oceans with adjustable waves
- Tune wave height, direction, speed, length, and steepness.
- Use repeatable, project-seeded wave patterns instead of unrelated motion on every frame.
- Adjust how the moving water surface appears under the scene's lighting and shadows.

### Contact with land and objects
- Create impact ripples when supported colliders enter or strike water.
- Produce trailing wakes, surface disturbances, and localized foam as Actors, Cells, and Characters move through water.
- Add breaking-wave foam and a thin shoreline wash that advances and recedes across reachable beach areas.
- Darken and highlight recently washed ground; wetness fades as the water recedes.

### More than an ocean
- Shape **Lakes and Rivers** with editable water paths, control points, and per-body settings.
- Tune their wave and appearance controls separately from the ocean.
- Apply underwater fog and color where the camera's view passes beneath water. Ocean effects use the animated surface and actual water coverage, rather than a simple camera-height switch.

Water effects are available in the editor; some visual behavior and performance limits are still being refined.

## Materials, lighting, and atmosphere

### Materials · BETA for newer authoring tools
- Create project-owned **Surface, Water, and Sky Materials** and reuse them across compatible objects.
- Set colors, texture references, roughness, and metallic response.
- Combine named material layers with opacity, reordering, and Normal, Multiply, Add, and Screen blend modes.
- Build connected Material Graphs using typed nodes and supported custom math functions.
- Preview edits, use Undo/Redo, and recover unsaved material work.
- Animate supported Material properties from AeoScript, including layer opacity.

### Lighting and scene appearance
- Configure directional and ambient lighting, fog, and sky appearance.
- Use custom Sky Materials to control gradients and colors.
- Adjust the **Horizon Mirage** effect for distant heat-shimmer-like distortion (**BETA**).
- Tune water response to existing world lighting without needing a separate lighting setup for the water.

## Actors, animation, and physics

### Imported objects and characters
- Import GLB models as Actors, including compatible animated character packages.
- Position, rotate, scale, duplicate, and organize placed objects.
- Assign project Materials and other compatible assets through drag-and-drop.
- Choose or create reusable Character and Camera Controller assets in Project Settings (**BETA**).

### Animation and gameplay attachments
- Select and play supported character animation clips, including layered animation actions.
- Attach items to supported points on a character rig and remove them during gameplay.
- Control character movement and camera behavior through project scripts.

### Physics and navigation
- Add solid objects, moving bodies, gravity, collisions, and contact-driven interactions.
- Test character movement and terrain contacts in Play mode; newer terrain collision work is **BETA**.
- Build navigation data and request paths for Characters and other supported game objects.
- Inspect navigation results and configure agent dimensions, traversal, and slope limits.

## AeoScript and gameplay

AeoScript is the gameplay language built into Worldkiln. Write `.aeo` files in the integrated editor, connect them to objects, and call the engine's APIs.

- **Scripts and Modules:** Organize game-wide behavior, reusable functions, per-object state, and entity lifecycle actions.
- **Events:** Respond to input, contact, gameplay events, and script-to-script messages.
- **World access:** Query and modify supported Actors, Cells, Characters, cameras, UI elements, and audio.
- **Timing and state:** Use `wait()`, collections, functions, closures, and persistent `save` values.
- **Advanced APIs (BETA):** Sample terrain surfaces, find and change Actor tags, subscribe to property changes, and animate supported Material properties.
- **Script tools:** Integrated errors and diagnostics, source navigation, and Play-mode testing.

[Learn AeoScript](AEOSCRIPT.md) · [Language guide](https://aeowun.com/aeoscript/) · [API reference](https://aeowun.com/docs/reference/)

## Interfaces, audio, and media

### UI authoring
- Build in-game interfaces with Panels, Text, and Buttons.
- Arrange elements with anchors, alignment guides, snapping, resize handles, and parent-child hierarchy editing.
- Preview common screen sizes and custom resolutions, with optional safe-area guides.
- Use presets for dialogue, prompts, top bars, quest panels, and inventory-like layouts.
- Connect supported UI events and properties to AeoScript.

![Worldkiln's UI authoring workspace](SCREENSHOTS/39-ui-editor-workspace.png)

### Sound and video
- Import and manage compatible audio files and position Audio Emitters in the scene.
- Play sounds and control supported audio behavior through AeoScript.
- Import supported video media for compatible media playback workflows (**BETA**).

## Particles and effects

- Place ParticleEmitters, including fire and smoke presets, and configure their emission behavior.
- Trigger bursts and control supported emitter state from AeoScript.
- Use imported native effects in the existing renderer, with Effects assets assigned by drag-and-drop (**BETA**).
- Browse optional Effects packs: **Essentials**, **Magic**, **World and Atmosphere**, and **Combat and Weapons**, currently containing **64 effects** across the collections (**BETA**, separate download where offered).

An AeoFX authoring/compiler foundation is under development; a complete public-facing effects editor should not be assumed from the available asset and playback tools.

## The editor and asset library

- Switch between world building, landscaping, scripting, UI creation, and project settings.
- Use **Project Explorer** and **Properties** to work with selected objects and project organization.
- Import and manage project-owned models, textures, audio, video, scripts, controllers, and other supported assets.
- Browse project content, search for assets, and drag compatible files into the World or an assignment field.
- Customize the editor theme, accent, and display preferences.
- Use the built-in AI assistant for supported project and script assistance (**BETA**, with permission controls).
- Record performance with **Development Test Run**, including frame statistics and timing for editor panels and game systems (**BETA**).

![Worldkiln editor with the viewport, Project Explorer, and Properties](SCREENSHOTS/02-editor-overview.png)

## Play, testing, and publishing

- Start **Play mode** to test movement, physics, scripts, effects, audio, cameras, and UI together.
- Keep Play-session changes separate from the authored scene.
- Store game progress with AeoScript's save API.
- Use **Build Game** to produce a **standalone Windows game**, independent of the editor.
- Review issues using Output, scripting diagnostics, and available development reports.

## See Worldkiln in action

The [scene and editor gallery](SHOWCASE.md) contains screenshots taken from Worldkiln, including constructed environments and UI authoring.

Worldkiln currently targets **Windows**. **BETA** means a feature is included and usable but may have bugs, limitations, or changes as it is refined. Feature availability can depend on the installed version and optional asset packs.

[Download Worldkiln](https://github.com/Aeowun/Worldkiln/releases) · [User guide](DIVE.md) · [Support](https://aeowun.com/support/)
