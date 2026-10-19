# Worldkiln features

[Download](https://github.com/Aeowun/Worldkiln/releases) · [User guide](DIVE.md) · [Documentation](https://aeowun.com/docs/)

Worldkiln combines a 3D world editor, AeoScript gameplay, and standalone Windows game builds.

## Worlds and environments

**Cells** are Worldkiln's building pieces for structures, platforms, interiors, and detailed objects. Place, erase, select, transform, and edit them directly in the viewport.

**Landscaping (BETA)** provides terrain generation, sculpting, smoothing, flattening, and material painting. Combine landscape layers with shorelines and atmospheric settings.

**Water (BETA)** responds to the scene: animated waves, collision-driven ripples and wakes, shoreline wash and foam, surface wetness, and a camera effect that follows the water surface when submerged.

**Materials and lighting** control the look of Cells, Actors, landscapes, skies, and water. Distance fog and particle effects add atmosphere. Newer material-authoring tools are marked BETA.

## Actors and assets

Import **GLB models** and place them as Actors. Arrange and transform props, configure collisions, and use animated Character packages.

The **Asset Manager** and **Content Browser** organize models, textures, audio, media, scripts, and project assets. Asset fields support drag-and-drop where available in the editor.

### Worldkiln 1.0 asset management — in development

The upcoming asset-management system uses **one project-owned asset registry**, so Asset Manager, Content Browser, and runtime share persistent asset identities. Assets appear in consistent categories rather than being organized according to the source folder used to import them.

- **One GLB, one Actor:** a model with multiple meshes remains a single Actor. Compatible rigged or animated Actors can be configured as Characters separately.
- **Project-owned imports:** install chosen AssetPacks or import external GLBs and supported media into the project, without requiring shared pack caches at runtime.
- **Consistent browsing:** previews, selection, inspection and dependencies refer to the same registered assets across both interfaces.
- **Creator-controlled organization:** optional custom groups do not change asset identity or type.
- **Asset management:** import review, missing-resource reporting, replacement and dependency-aware removal are part of the developing workflow.

**This is work toward Worldkiln 1.0, not a feature list for the current 0.8.5 public release.** The 0.8.7 release candidate is not yet approved as the next public executable.

## Gameplay

**AeoScript** connects gameplay to the World: object queries, input, events, movement, camera control, UI, audio, animation, and game state. Scripts can be reused as Modules or attached to game objects.

Worldkiln provides **physics and navigation** for moving Characters and Actors, collision events, path queries, and navigation-driven behavior.

Create **game UI** with Panels, Text, and Buttons. Add audio emitters and particle effects to scenes, and use AeoScript to control their runtime behavior.

Game progress can be stored through AeoScript's `save` APIs, separately from the scene saved in the editor.

## From editor to game

Use **Play mode** to test movement, collisions, scripts, cameras, and interfaces. **Build Game** creates a standalone Windows game from the project.

For instructions, see the [user guide](DIVE.md). For scripting details, see [AeoScript](AEOSCRIPT.md) and the [API reference](https://aeowun.com/docs/reference/).

**BETA** marks tools under active development.
