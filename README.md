# Worldkiln

**Worldkiln is a Windows game engine and editor for creating 3D games.** Build with Cells, sculpt landscapes, import 3D assets, script gameplay in AeoScript, and package standalone Windows games.

[Download](https://github.com/Aeowun/Worldkiln/releases) · [Getting started](https://aeowun.com/worldkiln/getting-started/) · [Documentation](https://aeowun.com/docs/) · [AeoScript](AEOSCRIPT.md)

## Features

- **World building:** Construct with Cells or sculpt and paint larger landscapes. Set up materials, lighting, skies, and fog.
- **Reactive water (BETA):** Directional waves, collision-driven ripples and wakes, shoreline foam, wet sand, and underwater camera effects.
- **Actors and animation:** Import GLB models, arrange props, and work with animated Characters.
- **Gameplay:** Write AeoScript for input, interactions, physics, navigation, UI, audio, and cameras.
- **Editing and testing:** Manage project assets, build interfaces, and test gameplay in Play mode before creating a standalone game.

See the [feature overview](FEATURES.md) for more detail.

## Getting Worldkiln

Worldkiln runs on **Windows 10 and 11**. Download the latest installer and optional assets from [Releases](https://github.com/Aeowun/Worldkiln/releases).

Open Worldkiln, create a project, and start building in the editor. The [getting started guide](https://aeowun.com/worldkiln/getting-started/) covers the first project.

## Documentation

- [User guide](https://aeowun.com/worldkiln/dive/) — build a scene, import assets, test, and package a game.
- [AeoScript](AEOSCRIPT.md) — gameplay scripting and language-specific concepts.
- [API reference](https://aeowun.com/docs/reference/) — functions, properties, and examples.
- [Changelog](CHANGELOG.md) — release history and development features.

# Supported Imports in Worldkiln 1.0.0

Worldkiln allows you to import your own 3D models, characters, animations, textures, sounds, videos, and visual effects into your projects.

This guide explains which file formats are supported and how to prepare assets for importing.

## 3D Models and Characters

| Format | Supported | Description |
|---|---|---|
| `.glb` | Yes | 3D models, characters, rigs, and animations |
| `.fbx` | No direct import | Convert to GLB first |
| `.obj` | No direct import | Convert to GLB first |
| `.gltf` | No direct import | Export as GLB instead |
| `.blend` | No direct import | Export from Blender as GLB |

**GLB is the recommended format for 3D assets.**

A GLB file can contain a model's geometry, materials, textures, skeleton, and animations in one file.

Worldkiln supports importing static models and animated characters, including animation clips and character rig information. More complex characters may require animation or skeleton adjustments after importing.

## Images and Textures

| Format | Supported |
|---|---|
| `.png` | Yes |
| `.jpg` | Yes |
| `.jpeg` | Yes |
| `.webp` | Not supported by the standard media importer |
| `.svg` | Not supported by the standard media importer |

Use images for materials, textures, interface graphics, and other visual content.

PNG is a good default, particularly for textures requiring transparency.

## Audio

| Format | Supported |
|---|---|
| `.wav` | Yes |
| `.mp3` | Yes |
| `.ogg` | Available in some asset-management workflows |

Use WAV or MP3 for the most consistent importing experience.

These files can be used for sound effects, music, and other game audio.

## Video

| Format | Supported |
|---|---|
| `.mp4` | Yes |

MP4 is supported for imported video media. Playback compatibility may depend on the video encoding and target platform.

## Visual Effects

| Format | Supported | Description |
|---|---|---|
| `.efkefc` | Yes | Effekseer effect file |
| `.efkpkg` | Yes | Packaged Effekseer effect and associated resources |

Visual effects can include particles, fire, smoke, explosions, and other animated effects.

## Asset Packs and Folders

Worldkiln supports importing folders containing compatible assets, allowing multiple files to be brought into a project.

Some workflows also support packaged assets, including Effekseer effect packages.

**Important:** A ZIP archive is not automatically interchangeable with a GLB model or a Worldkiln asset package. If an asset pack contains FBX or OBJ models, convert those models to GLB before importing.

## Converting Unsupported Models

If you download a model in FBX, OBJ, or Blender format, you can convert it using [Blender](https://www.blender.org/).

1. Open Blender and import the model.
2. Check its textures, orientation, and animations.
3. Choose **File → Export → glTF 2.0**.
4. Select **glTF Binary (`.glb`)** and export.
5. Import the resulting GLB into Worldkiln.

## Additional Information

Not every asset using a supported file extension is guaranteed to import successfully. Models with unsupported features, missing textures, incompatible animation data, or invalid files may require adjustments.

For source details and current development, visit the [Worldkiln GitHub repository](https://github.com/Aeowun/Worldkiln).

## Support and license

Report problems through [AEOWUN Support](https://aeowun.com/support/).

Worldkiln is distributed under the [Worldkiln Source-Available License](LICENSE). This repository contains public releases and documentation.

Features marked **BETA** are under active development.
