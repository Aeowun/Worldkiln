# Hearthvale AeoScript integration fixture

This directory is a tracked copy of the complete Hearthvale script corpus from:

`%LOCALAPPDATA%\Aeowun\UserData\Hearthvale\scripts`

Keep the relative paths and script contents synchronized when Hearthvale gameplay scripts change. The `hearthvale_full_script_suite_starts_and_handles_gameplay_events` test deliberately loads the scripts together, attaches the same Actor Modules used by the authored world, starts the scene, and dispatches representative UI/gameplay events.

The fixture exists to catch cross-Module scope, function ownership, coroutine, event-arity, UI, and save-state integration failures that isolated parser or interpreter tests cannot expose.
