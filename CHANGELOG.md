# Worldkiln Changelog

Development history for Worldkiln and AeoScript.

## Contents

- [1.0.x](#10x)
- [1.1.x](#11x)

---

# 1.0.x

## 1.0.0
- Initial public release of Worldkiln V1.0.0

## 1.0.1

> Worldkiln V1.0.1 introduces AEOWUN account integration and direct game publishing, alongside improvements to Editor stability, physics, lighting, and standalone game reliability. This update makes it easier to build, test, and share games while delivering a more consistent experience between the Editor and exported games.

### Editor Improvements

- Added AEOWUN member sign-in and game submission for review from the Editor.
- Added the latest Worldkiln developer notes to the Home screen.
- Added Publish to Aeowun beneath Manage for built games on the Home screen.
- Fixed the Packaging panel scrolling to keep publishing controls accessible.
- Fixed asset pack downloads to use the dedicated Worldkiln asset-pack release repository.
- Reworked mouse capture and cursor handling for more reliable behavior in the Editor and during gameplay.
- Improved keyboard and mouse input handling when switching between game windows and other applications.
- Fixed mouse capture and cursor visibility issues when opening menus or returning to gameplay.
- Fixed Actors appearing too high or too low when placed or moved in the Editor.
- Fixed collision issues affecting rotated Actors and fast-moving Characters.
- Fixed Actors taking incorrect routes, missing turns, or continuing to move after reaching their destination.
- Fixed unwanted dark shadows appearing on Cells.
- Improved point-light shadows for more consistent lighting.
- Added Find & Replace to the AeoScript Editor, with current-script and project-wide `.aeo` search, match navigation, and replacement controls.
- Expanded the input-action foundation with multiple bindings, analog deadzones, and improved keyboard and mouse focus handling.
- Improved keyboard and mouse input release handling when interface controls consume events.

### Runtime and AeoScript
- Improved consistency between `game.restart()` and `save.restart()`, separating gameplay restart from saved-data reset.
- Improved runtime save-root configuration for isolated game environments.

### Installation
- Fixed development-built installers being eligible for executable self-cleanup during installation.

### Standalone Game Improvements

- Fixed issues that prevented some scripts and gameplay features from starting correctly in exported games.
- Improved collision and physics consistency between Editor Play and standalone games.
- Fixed issues with object attachments, material changes, and water movement in exported games.
- Fixed game restart functionality not working correctly in standalone games.
- Improved visual consistency, including shadows and interface scaling.
- Added better error logging to make troubleshooting exported games easier.
+


### AeoScript Game Controls

- Added game.quit() and game.restart() to control the current game instance, with saved values preserved when restarting.
- Added game.pause() and game.resume() for single-player games.
- Added game quit, pause, and resume events for creator-authored lifecycle handling.
- Added game timing, pause-state, and Editor/standalone context properties.

# 1.1.x

## 1.1.0

> Multiplayer features are undergoing release verification. Security, compatibility, and end-to-end internet gameplay checks.

### Multiplayer

- Added host/client networking sessions with versioned connections and disconnect handling.
- Added player ownership so each connected player controls their own Character.
- Synchronized Character movement, rotation, and facing direction across connected games.
- Preserved existing Character controls and independent camera behavior for each player.
- Added support for multiple players joining the same multiplayer session.
- Added synchronization for shared Actors and their physics, including movement, rotation, and velocity.
- Added synchronization for Cells created, moved, modified, or removed during gameplay.
- Improved multiplayer stability when players disconnect or join an existing session.
- Added server-authoritative handling of shared physics to keep objects consistent between players.
- Added tools to launch connected host and client instances from the Editor.
- Added multiplayer role and player identity APIs, client requests, and host-published game state.
- Added diagnostics when client scripts attempt operations requiring host authority.
- Strengthened host-side player movement speed and collision validation.
