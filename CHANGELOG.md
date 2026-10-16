# Worldkiln Changelog

Development history for Worldkiln (formerly AeoEngine) and AeoScript.

> Worldkiln is the current product name. Historical release entries retain AeoEngine where that was the product name at the time.

## Contents

- [0.1.x — Foundation](#01x--foundation)
- [0.2.x — Editor Expansion](#02x--editor-expansion)
- [0.3.x — Physics](#03x--physics)
- [0.4.x — Blocks and Building](#04x--blocks-and-building)
- [0.5.x — Character and Gameplay](#05x--character-and-gameplay)
- [0.6.x — Editor and AeoScript Foundation](#06x--editor-and-aeoscript-foundation)
- [0.7.x — Gameplay and Scripting Expansion](#07x--gameplay-and-scripting-expansion)
- [0.8.0 — UI, Actors, Attachments, and Installation](#080--ui-actors-attachments-and-installation)
- [0.8.5 — Particle effects and Entity control](#085--particle-effects-and-entity-control-public-release)
- [More features included in Worldkiln](#more-features-included-in-worldkiln--beta-where-marked)
- [Hearthvale Expansion](#unreleased--hearthvale-expansion)

---

# 0.1.x — Foundation

## 0.1.0

### Added

- Initial voxel World.
- Basic World editing.
- Block building and erasing.
- Editor camera and navigation controls.

---

## 0.1.1

### Added

- Project and World saving.
- Initial Home screen and project management.

---

# 0.2.x — Editor Expansion

## 0.2.0

### Added

- Cell selection and Properties editing.
- Docked editor panels.
- Multiple building grid planes.
- Expanded editor camera and viewport controls.

---

## 0.2.1

### Added

- Light cells.
- Persistent World lighting.

---

# 0.3.x — Physics

## 0.3.0

### Added

- Play mode physics.
- Configurable World gravity.
- Dynamic physics bodies.
- Collision between moving bodies and solid World geometry.
- Sleeping and waking for supported physics bodies.
- Separation between editing and Play mode.

### Fixed

- Improved resting stability.
- Improved collision response.
- Fixed bodies being continuously pushed into supporting surfaces.
- Fixed Light cells being treated as physics geometry.

---

## 0.3.1

### Fixed

- Improved stacked-body stability.
- Improved sleeping and waking behavior.
- Fixed bodies failing to wake when support was removed.

---

# 0.4.x — Blocks and Building

## 0.4.0

### Added

- `Block` as the primary building cell.
- Per-Block color.
- Configurable Build properties.
- RGB editing and color picker.

---

## 0.4.1

### Added

- Drag building and erasing.
- Line building.
- Plane building.
- Build and erase previews.

---

## 0.4.2

### Fixed

- Fixed Block colors being lost during Play mode physics.
- Fixed Block visibility being lost when physics took control.
- Fixed Color Picker interaction issues.

---

# 0.5.x — Character and Gameplay

## 0.5.0

### Added

- Character system.
- Character movement and jumping.
- Gravity and grounded state.
- World collision.
- Spawn Points with clearance checking.

---

## 0.5.1

### Added

- 3D character rig and skeleton.
- Idle and Walk animations.
- Animation blending.
- Character appearance customization.
- Character facing based on movement direction.

---

## 0.5.2

### Added

- Third-person gameplay camera.
- Camera orbit and pitch controls.
- Camera collision with World geometry.
- Character-follow camera behavior.

---

## 0.5.3

### Fixed

- Editor camera state is preserved when entering and leaving Play mode.
- Editor camera controls no longer interfere with gameplay.
- Improved gameplay-camera obstruction handling.

---

# 0.6.x — Editor and AeoScript Foundation

## 0.6.0

### Added

- Expanded World hierarchy.
- Multi-cell selection.
- Plane and Top-Block selection modes.
- Surface-aware building.
- Editor shortcuts.
- Undo and Redo for World editing.
- Selection focus.
- Project texture importing.
- Project-owned texture assets.
- Texture loading and caching.

---

## 0.6.1

### Added

- First AeoScript language implementation.
- Variables, constants, numbers, strings, booleans, and `nil`.
- Arithmetic and logic.
- `if`, `while`, and `for`.
- Functions and event handlers.
- Cooperative `wait()` support.
- Integrated Script Editor.
- Output terminal with warnings and errors.

---

## 0.6.2

### Added

- Script access to Cells and Entities.
- Stable Cell IDs.
- `find()` and Cell discovery APIs.
- Script-controlled Cell properties during Play mode.
- Script bindings for World objects.

### Changed

- Scripts now reference Cells by stable IDs instead of coordinates.
- Changes made during Play mode no longer modify the saved World.

---

## 0.6.3

### Added

- AeoScript `math` library.
- AeoScript `basket` collection library.
- AeoScript `string` library.
- Unicode-aware string operations.

---

## 0.6.4

### Added

- Script bindings now target unique Cell IDs.
- Multiple Cells can share the same name without breaking script bindings.

### Fixed

- Fixed incorrect script-binding warnings on non-Entity Cells.

---

## 0.6.5

### Added

- Nested AeoScript function calls can now use `wait()`.
- Improved function call-stack handling.
- Improved Map behavior.

---

## 0.6.6

### Fixed

- Fixed script fields losing changes after a function yielded or completed.
- Script state now persists correctly between lifecycle callbacks and updates.

---

# 0.7.x — Gameplay and Scripting Expansion

## 0.7.0

### Added

- Custom Cell Attributes using Number, Bool, and String values.
- Script access to Cell Attributes.
- Top-level AeoScript statements.
- Top-level scripts can use `wait()`.
- Improved script error reporting.

---

## 0.7.1

### Added

- Scripts can change Cell Attributes during Play mode.
- `math.random()`.
- `cell.new()` for creating temporary Cells during gameplay.
- `cell.delete()`.
- Script control over temporary Cell position, name, visibility, solidity, anchoring, and color.

### Changed

- Play-mode Cell changes remain temporary and reset when Play mode ends.

---

## 0.7.2

### Added

- Sky environment system.
- OpenGL cubemap skyboxes.
- Temperate, Tropical, Desert, Snowy, and Mars sky presets.
- World controls for enabling and selecting skies.

### Fixed

- Fixed script state leaking across Play → Stop → Play.
- Fixed nested function return values reaching the wrong call site.
- Fixed release builds resolving `UserData` and `.assets` from the wrong location.

---

## 0.7.3

### Added

- Script-defined objects with fields, constructors, and methods.
- Lexical closures.
- Script access to keyboard and mouse input.
- Script-defined player controllers.
- Script-defined gameplay cameras.
- Script enable and disable controls.
- `on_touch` and `on_overlap` collision events.
- Runtime UI with Panels, Text, and Buttons.
- Script-created UI controls and button callbacks.
- Mouse cursor visibility and screen-lock controls.
- Audio Emitters.
- WAV audio assets.
- 3D audio playback.
- Script control over sound playback, looping, and volume.
- Improved AeoScript syntax highlighting.

### Changed

- Player movement and camera behavior can be controlled through AeoScript while AeoEngine continues handling physics and collision.
- UI is positioned relative to the playable viewport.
- Controller, camera, UI, and audio state reset when Play mode ends.

### Fixed

- Fixed editor projection and picking offsets caused by side panels.
- Fixed gameplay UI rendering over editor panels.
- Fixed script state not resetting correctly after Play mode.
- Fixed controller and camera state being lost between updates.

---

## 0.7.4

### Added

- Exposed-face voxel meshing.
- Chunked static World meshes.
- Dirty-chunk rebuilding.
- Greedy voxel meshing.
- Copy and Paste.
- Multi-selection drag movement.
- Additive selection with `Ctrl`.
- Drag previews with collision indicators.
- Generic scripted character controls.
- Scripted character spawning.
- Entity-based touch callbacks.
- Improved World hierarchy navigation for Lights, Audio Emitters, Spawn Points, FX Blocks, Players, and NPCs.
- Large Block collections are shown as a count instead of thousands of hierarchy entries.

### Changed

- Static World rendering now uses chunked meshes and selective rebuilding.
- Copying a multi-selection uses its lowest cells as the placement pivot.
- Undo and Redo now preserve script bindings with copied and moved World content.
- Character movement decisions can be scripted while AeoEngine continues handling physics, gravity, and collision.

### Fixed

- Fixed shadows becoming detached from Blocks as the camera moved.
- Fixed scripted Entity character controls.
- Fixed gameplay event dispatch.

## 0.7.5 — Runtime and Editor Maturity

### Added

- Configurable editor input bindings for camera navigation, orbit, free look, selection, and deletion.
- Persistent editor Settings for camera behavior, undo history, Autosave, panel visibility, and editor preferences.
- Editor Theme controls including light/dark presentation, accent presets, interface scaling, font sizing, panel styling, separators, animations, tooltips, and status-bar visibility.
- Generic AeoScript keyboard events through `on.keypress(key, down)`.
- Keyboard polling through `input.is_key_down(key)`.
- First-class authored Actors with independent IDs, persistence, properties, package selection, physics defaults, attributes, and script bindings.
- Generic AeoScript Modules for reusable script-to-script communication.
- Actor-attached Modules with independent per-Actor state and `self.parent`.
- Project and Player Actor lookup APIs.
- Standalone game builds with a dedicated runtime executable and packaged project data.
- Built-game discovery and launching from the Home screen.
- Resizable Output dock that preserves Play logs after returning to the Editor.

### Changed

- Editor input and appearance settings are now persistent application preferences rather than project data.
- Actors are formally separated from voxel Cells and become runtime Entities during Play mode.
- Standalone games run without requiring editor-only systems.
- AeoScript runtime cleanup and error handling were hardened across scripts, callbacks, events, handles, and runtime objects.

### Fixed

- Fixed destroyed Entity handles causing `entity.is_valid()` failures.
- Fixed invalid handle types being accepted by `cell.delete()`.
- Fixed queued script event errors being silently discarded.
- Fixed callbacks and dynamic properties surviving after their owning script, Entity, or Cell was removed.
- Fixed `on_destroy()` failures being lost during runtime teardown.
- Fixed disabled Actor Modules being executed incorrectly.
- Fixed nested AeoScript Module calls losing caller context across normal and yielding calls.
- Fixed standalone builds launching without the complete authored World and gameplay runtime setup.
- Fixed built-game discovery, packaged working directories, and standalone `Escape` handling.
- Fixed Actor migration and persistence issues when transitioning from the older Actor-as-Cell model.
- Fixed invalid and stale point-light entries.

---

# 0.8.x — Engine Maturity

## 0.8.0 — UI, Actors, Attachments, and Installation

### Added

- Responsive UI anchors for edges, corners, centers, and stretched layouts with margins.
- UI Authoring resize handles, editable names, stable script keys, anchor selection, and Undo/Redo.
- `ui.find()` and `ui.get()` for locating authored UI controls from AeoScript.
- Shared authored/runtime UI anchor and key properties.
- Standalone Windows installation and automatic repair of required Worldkiln application files.
- Runtime rig attachment-point lookup for package-backed Actors.
- Play-mode attachment system with named attachment points.
- Generic AeoScript `attach()` and `detach()` operations.
- Layered one-shot character animations that can play over locomotion.
- Experimental voxel GI controls and radiance-cache research tools.

### Changed

- Authored and runtime UI layout now resolves against the playable viewport.
- Actor selection bounds now follow rendered package geometry.
- Output dock sizing is preserved across editor workspaces and hide/show cycles.

### Fixed

- Authored UI now loads before scripts start, allowing startup scripts to find authored controls.
- Deleting a UI parent preserves the resolved placement of its children.
- Moving UI elements no longer changes their parent implicitly.
- Runtime UI no longer shifts or shrinks beneath editor panels.
- Actor Module startup failures are isolated so one broken Actor does not stop unrelated gameplay systems.
- `on_touch` now fires when contact begins instead of every physics step while contact continues.
- Character-backed Actor transforms and collision bodies remain synchronized.
- Package aliases allow renamed Actor packages to preserve existing authored references.
- Unknown package IDs now report an error instead of silently using a fallback Character.
- Gameplay keys no longer trigger conflicting editor shortcuts during Play mode.
- Script diagnostics were moved into a resizable, closable Problems window.

---

## 0.8.1 — Navigation

### Added

- Engine-level navigation for static voxel Worlds.
- Walkable-space extraction based on supported voxel surfaces.
- Configurable navigation-agent width, height, step-up, and step-down limits.
- Deterministic A* pathfinding.
- Navigation diagnostics and viewport visualization.
- Persistent **Show Navmesh** viewport diagnostic.
- Engine-owned Actor path following with movement, facing, and locomotion animation.
- AeoScript Actor navigation:
  - `navigate_to(position, speed?, stop_distance?)`
  - `cancel_navigation()`
  - `is_navigating()`
- `navigation_finished(actor)` events.
- `navigation_failed(actor, reason)` events.
- Actor callbacks for navigation completion and failure.
- Stuck-route detection.

### Changed

- Existing Character and Entity pathfinding now uses the shared navigation system.
- Runtime routes are compacted into safe movement waypoints while full paths remain available for diagnostics.
- Navigation snapshots are reused until World geometry changes.
- Actor Modules that call `wait()` during startup now resume through the normal script scheduler.

### Fixed

- Fixed path coordinates behaving inconsistently on opposite sides of the World origin.
- Fixed yielded Actor Modules being discarded during initialization.
- Fixed completed navigation bypassing Actor Module callbacks.
- Fixed completed or cancelled navigation leaving forced movement animation or velocity behind.
- Fixed Project Explorer search expanding the left sidebar while typing.
- Fixed Actor package selection reverting after reopening editor controls.

---

## 0.8.2 — Save State, Worldkiln Identity, and Actor Physics

### Added

- Project-specific typed save state.
- AeoScript `save` API:
  - `save.get()`
  - `save.set()`
  - `save.has()`
  - `save.remove()`
  - `save.commit()`
- Persistent `number`, `bool`, and `string` save values.
- Explicit save commits with atomic file replacement.
- Project-isolated save-data storage.
- Canonical Worldkiln executable, package, and installed application identity.
- Automatic Windows Desktop shortcut creation.
- Installer completion screen with explicit Launch and Close options.
- Authored Actor rotation and scale controls.
- Authored Actor collider size and offset controls.
- Package-derived default Actor colliders.
- Solid Actor-to-Character collision.
- Character grounding on solid Actors.
- Unified Text and Button typography properties:
  - font size
  - horizontal alignment
  - vertical alignment
  - wrapping
  - padding
  - line spacing

### Changed

- Save changes remain in memory until `save.commit()` succeeds.
- Play mode and standalone games load project save state independently from authored World data.
- Worldkiln is now the canonical product identity throughout the executable and installation workflow.
- UI text uses the same layout behavior in the authoring canvas and game runtime.

### Fixed

- Fixed runtime UI parenting rejecting handles returned by `ui.new()` and `ui.find()`.
- AeoScript runtime errors now identify the active script, function, and source line where available.
- Nested Module errors now identify the Module where the failure occurred.
- Fixed repeated installer launches from an already-installed Worldkiln executable.
- Fixed installer self-copy and cleanup edge cases.
- Installer failures now display their underlying error.
- Fixed Actor colliders ignoring authored or package dimensions.
- Fixed Characters walking through solid package-backed and custom Actors.

---

## 0.8.3 — UI Authoring Usability

### Added

- Reorganized UI Inspector sections for Identity, Layout, Appearance, Typography, and Interaction.
- UI preview resolutions for:
  - `1280x720`
  - `1600x900`
  - `1920x1080`
  - custom dimensions
- Optional 95% and 90% safe-area guides.
- Authoring-canvas selection bounds and resize handles.
- Anchor, parent-bound, and snapping visualization.
- Center, edge, and sibling snapping with visible guides.
- Hierarchy drag-and-drop reparenting with cycle prevention.
- Root reparenting and recovery access for invalid or unattached UI nodes.
- Undo/Redo coverage for UI document changes.
- Duplicate, rename, delete, bring-forward, and send-backward actions.
- Editable UI presets:
  - Dialogue Panel
  - Top Bar
  - Quest Panel
  - Satchel Panel
  - Center Prompt
- Inline validation for invalid or duplicate script keys, dimensions, parents, and hierarchy cycles.
- Linked and independent margin editing for stretched elements.

### Changed

- UI authoring operations now use the selected preview resolution instead of the current editor-panel dimensions.
- Legacy Buttons migrate to centered typography where no explicit alignment was authored.
- UI text is clipped to the padded bounds of its element in both the editor and runtime.

### Fixed

- Fixed dropped UI elements unexpectedly becoming children of the container beneath the cursor.
- Improved UI parenting errors for invalid handle types.
- Fixed same-named functions in separate AeoScript Modules sharing compiled state.
- Fixed nested Module calls and object constructors losing field changes during synchronization.

---

## 0.8.5 — Particle effects and Entity control (public release)

### Added

- **Particle effects:** authored ParticleEmitter Cells, configurable fire and smoke presets, billboard particle rendering, emitter controls, and AeoScript operations to burst, toggle, clear, and count particles. Emitter settings persist with World data; particle simulation runs during Play.
- **Entity visibility and activation:** generic AeoScript Entity handles expose `visible` and `enabled`, including Entities behind authored Actors.

### Improved

- Continued the Worldkiln product rename across the editor and scripting integration while retaining compatibility with existing AeoScript projects and stored data.

---

## More features included in Worldkiln — BETA where marked

> **BETA features are included in the engine and available to use.** BETA means they are still being refined, so bugs, limitations, or changes are possible. Established features do not become BETA simply because they receive improvements.

### Editor and workflow

- **Redesigned editor workspace:** cleaner Home viewport, dedicated Script, UI and Terrain workspaces, contextual Properties, direct Project Settings access, and a searchable feature directory.
- **Unified Project Explorer:** project-named hierarchy with user folders, Actors, Scripts, scene objects, and a separate Player Scripts section; drag-and-drop organization and safer selection workflows.
- **Expanded Content Browser:** folder tree, breadcrumbs, scoped search, asset previews, and drag-to-place Actor packages.
- **Faster building:** common block and Actor properties available directly in Build mode; selecting an object reveals its relevant Inspector settings.
- **Theme customization:** additional Cyan and Aeo Emerald accent choices.

### Terrain and rendering

- **Layered landscaping:** multiple terrain layers, generator workflows, sculpting, painting, and terrain persistence improvements.
- **More terrain controls:** procedural grass, beach/sand painting, water and underwater presentation, atmospheric distance fog, and terrain material improvements.
- **Rendering performance:** improved editor/Play rendering paths, chunk-distance handling, indexed lighting, and reduced repeated work in large Worlds.
- **Navigation diagnostics:** increased the editor NavMesh snapshot overlay viewing range without changing pathfinding or actual World render distance.

### Scripting, AI, and assets

- **Integrated AI assistant:** source-aware project exploration and guarded editing tools for scripts and scene objects, with configurable approval modes, diagnostics, and per-file Keep/Revert controls.
- **AeoScript examples:** source-derived working examples and automated example-maintenance checks.
- **Media and asset handling:** improvements to imported 3D Actors, character packages, media playback, and asset management.
- **Character selection:** no forced robot/hero default; projects can explicitly choose **None** when no valid Character package is available.
- **Color-only building:** built-in **Block_txt** option returns Cells to an untextured, color-based appearance without requiring an image texture.
- **Project reliability:** improvements to save/load preservation, save-data editing, script diagnostics, and error isolation.

### Water, materials, and the World

- **Reactive water (BETA):** oceans and shorelines with configurable waves, moving foam, collision-driven ripples and wakes, and wet-looking surfaces that dry after waves recede.
- **Underwater camera effects (BETA):** camera effects respond to where the moving water surface covers the view, with underwater tint and fog.
- **Material creation (BETA):** Surface, Water, and Sky materials with saved layers and a Material Editor for shaping how the world looks.
- **Landscaping (BETA):** terrain generation, sculpt and paint tools, slope and shoreline materials, procedural grass, and water-level controls.
- **World presentation:** sky, lighting, and distance fog controls that help bring the scene together.

### Characters, assets, and effects

- **Project asset tools (BETA where marked):** fuller Asset Manager and Content Browser workflows, asset-pack discovery, imported 3D packages, and asset assignment by drag-and-drop.
- **Controllers and cameras (BETA):** create reusable project-owned controllers and camera assets from Project Settings.
- **Effects (BETA where marked):** particle and effect assets for scenes, with supported editor and script controls.

### Scripting and game creation

- **AeoScript extras (BETA):** terrain surface queries, Actor tags, property-change subscriptions, and supported material animation through tweens.
- **Script editing:** improved diagnostics, script navigation, and debugging tools for finding gameplay problems.
- **Standalone games:** updated Build Game packaging for a standalone Windows executable with game data and matching runtime assets.

For how to use these features, see the [Worldkiln User Guide](DIVE.md) and [AeoScript Guide](AEOSCRIPT.md). The feature list describes what's available, not a promise that every BETA feature is perfect.

---

## Unreleased — Hearthvale Expansion

### Added

- Playable **Roots Beneath the Wall** quest with:
  - Forester Elowen
  - persistent quest progression
  - marked-tree inspection
  - equipped hatchet gathering
  - tree depletion and respawning
  - Woodcutting XP
  - sawbench processing
  - Carpentry XP
  - permanent north-postern repair
- Playable **Catch for the Kettle** side quest with:
  - Fisher Elowen
  - Apple Pond fishing marker
  - equipped fishing rod
  - timed fishing
  - cooking interactions
  - persistent Fishing and Cooking XP
  - trout delivery rewards
  - Copper Kettle cooking fire
- Expanded Hearthvale Actor and asset library including:
  - forester, guards, storekeeper, fisher, farmer, blacksmith, cook, miner, merchant, and villagers
  - wolves, boars, deer, rabbits, chickens, cows, and sheep
  - gathering tools and tool tiers
  - weapons and shields
  - forestry and mining resources
  - crop growth stages and wild plants
  - crafting stations
  - town, farm, blacksmith, pond, postern, and wilderness props
- Expanded Hearthvale terrain and authored regions including routes, skyline terrain, an east-bank tower, ruins, and overlook areas.
- Editor and Play render-distance controls for voxel chunks.

### Changed

- Hearthvale now exercises a large multi-script gameplay flow spanning Actors, Modules, UI, save state, quests, gathering, processing, and rewards.

### Fixed

- Fixed nested AeoScript Module cache ownership issues.
- Fixed resumable expressions containing multiple Module calls.
- Nested Module failures now retain the correct source Module, function, and line information.
