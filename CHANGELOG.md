# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.16](https://github.com/davehorner/startt/compare/v0.1.15...v0.1.16) - 2025-06-01

### Fixed

- respect shake, border, and title options.

## [0.1.15](https://github.com/davehorner/startt/compare/v0.1.14...v0.1.15) - 2025-06-01

### Added

- ANSI codes enabled. detached mode in gui. update failed_hwnds.

## [0.1.14](https://github.com/davehorner/startt/compare/v0.1.13...v0.1.14) - 2025-05-30

### Added

- add new options for tracking windows in README; improve error handling and logic in GUI code

## [0.1.13](https://github.com/davehorner/startt/compare/v0.1.12...v0.1.13) - 2025-05-27

### Added

- add --gui option for launching the egui app

### Other

- untangle this mess.

## [0.1.12](https://github.com/davehorner/startt/compare/v0.1.11...v0.1.12) - 2025-05-24

### Added

- *(grid)* enable full monitor area fitting and improve console resizing; consoles (cmd.exe) are special and can't do extended sizes.

## [0.1.11](https://github.com/davehorner/startt/compare/v0.1.10...v0.1.11) - 2025-05-23

### Added

- *(cli)* add --debug-chrome option for URL rewriting

## [0.1.10](https://github.com/davehorner/startt/compare/v0.1.9...v0.1.10) - 2025-05-23

### Added

- Add --hide-taskbar and --show-taskbar options

### Other

- forgottten update to README.md

## [0.1.9](https://github.com/davehorner/startt/compare/v0.1.8...v0.1.9) - 2025-05-23

### Added

- Enhance grid placement for parent and child windows. --assign-parent-cell --reserve-parent-cell

## [0.1.8](https://github.com/davehorner/startt/compare/v0.1.7...v0.1.8) - 2025-05-23

### Added

- Add --fit-grid option for window placement and resizing

## [0.1.7](https://github.com/davehorner/startt/compare/v0.1.6...v0.1.7) - 2025-05-23

### Added

- add window manipulation options and shake duration

### Fixed

- lost content.

## [0.1.6](https://github.com/davehorner/startt/compare/v0.1.5...v0.1.6) - 2025-05-22

### Added

- add --timeout/-t option to close windows after specified seconds

### Other

- update README.md with additional usage examples and usage notes

## [0.1.5](https://github.com/davehorner/startt/compare/v0.1.4...v0.1.5) - 2025-05-21

### Fixed

- regression fix for non-follow url shaking.  -F --follow-forever for

## [0.1.4](https://github.com/davehorner/startt/compare/v0.1.3...v0.1.4) - 2025-05-20

### Added

- ctrl-c now kills all child processes. etw.  lib. filter out

## [0.1.3](https://github.com/davehorner/startt/compare/v0.1.2...v0.1.3) - 2025-05-20

### Added

- add grid layout support for window placement in main.rs and update usage in README.md

## [0.1.2](https://github.com/davehorner/startt/compare/v0.1.1...v0.1.2) - 2025-05-20

### Added

- add -f/--follow flag to shake child windows as they appear

## [0.1.0](https://github.com/davehorner/startt/releases/tag/v0.1.0) - 2025-05-08

### Other

- `startt` solves a long-standing Windows poor design/quirk: when you do
## [0.1.1](https://github.com/davehorner/startt/compare/v0.1.0...v0.1.1) - 2025-05-09

### Added

- *(gui-launch)* improve HWND detection, URL handler resolution, and window positioning
