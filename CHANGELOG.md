# Worldkiln Changelog

Development history for Worldkiln and AeoScript.

## Contents

- [1.0.x](#10x)

---

# 1.0.x

## 1.0.0
- Initial public release of Worldkiln V1.0.0

## 1.0.1

> This update improves Worldkiln's stability, object placement, physics, navigation, and lighting. It also brings exported games closer to the experience you get when playing inside the Editor.

### Editor Improvements

- Fixed Actors appearing too high or too low when placed or moved in the Editor.
- Fixed collision issues affecting rotated Actors and fast-moving Characters.
- Fixed Actors taking incorrect routes, missing turns, or continuing to move after reaching their destination.
- Fixed unwanted dark shadows appearing on Cells.
- Improved point-light shadows for more consistent lighting.

### Standalone Game Improvements

- Fixed issues that prevented some scripts and gameplay features from starting correctly in exported games.
- Improved collision and physics consistency between Editor Play and standalone games.
- Fixed issues with object attachments, material changes, and water movement in exported games.
- Fixed game restart functionality not working correctly in standalone games.
- Improved visual consistency, including shadows and interface scaling.
- Added better error logging to make troubleshooting exported games easier.
- Improved single-executable offline packaging