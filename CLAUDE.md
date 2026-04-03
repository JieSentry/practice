# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build Commands

```powershell
# Dioxus (recommended for UI)
dx build --release --package ui

# Standard cargo (backend works but UI may appear blank)
cargo build

# Tests
cargo test -- --nocapture

# Linting
cargo fmt --check
cargo clippy -- -D warnings
```

Windows-only: Requires Dioxus CLI, OpenCV (statically linked), and LLVM. Use vcpkg: `vcpkg install opencv4[contrib,nonfree]:x64-windows-static`

## Architecture

### Workspace Structure
- **`backend/`** - Core bot logic: game detection, player automation, ECS systems
- **`ui/`** - Dioxus desktop UI application
- **`platforms/`** - Platform-specific code (Windows only currently)

### Backend Architecture

**ECS Pattern** (`backend/src/ecs.rs`): The bot uses an Entity-Component-System architecture:
- `World` contains `MinimapEntity`, `PlayerEntity`, `SkillEntity` (array), `BuffEntity` (array)
- Each entity has a `state` (enum variant) and `context` (runtime data)
- `Resources` holds shared state: input, detector, RNG, operation mode, notification

**Game Loop** (`backend/src/run.rs`): Fixed 30 FPS loop (`FPS = 30`, `MS_PER_TICK = 33.33ms`):
1. Capture screen frame via `platforms`
2. Run detection via OpenCV/ONNX models
3. Execute system updates: `minimap::run_system`, `player::run_system`, `skill::run_system`, `buff::run_system`
4. `rotator.rotate_action` selects next action
5. Input is updated via `resources.input.update_tick()`

**UI-Backend Communication**: Uses tokio unbounded channels with a `Request`/`Response` pattern:
- `REQUESTS` static holds the channel
- `send_request!` macro handles request/response pairing
- Backend runs in separate tokio runtime, UI in Dioxus async context

**Detection Templates** (`backend/src/detect.rs`): Template matching via OpenCV for UI elements (popups, menus, buttons). Template images stored in `backend/resources/` with `*_ideal_ratio.png` naming.

**ONNX Models** (`backend/resources/*.onnx`): MobileNet-style detectors for:
- Mob NMS (non-maximum suppression)
- Rune detection and spin
- Minimap parsing
- Transparent shape (lie detector)
- Violetta boss
- Text detection/recognition

**Key Modules**:
- `player/` - All player states (Idle, Moving, Jumping, Falling, UsingSkill, CashShop, etc.)
- `solvers/` - Mini-game solvers: rune, shape, violetta
- `services/` - Mediator pattern for UI requests: capture, input, character, map, settings, database
- `notification.rs` - Feishu webhook notifications

### Platforms Architecture

Provides window capture and input simulation abstractions:
- `capture.rs` - Frame capture interface
- `input.rs` - Keyboard/mouse simulation
- `windows/` - Win32 API implementations (BitBlt, WGC capture, window enumeration)

### UI Architecture

Dioxus desktop app with tabbed interface:
- `ActionsScreen` - Bot action configuration
- `CharactersScreen` - Character management
- `SettingsScreen` - Bot settings
- `LocalizationScreen` - UI text localization
- `DebugScreen` (debug only) - Debug controls

Uses `Signal` for reactive state, `ContextProvider` for app-wide state.

## Important Notes

- **Edition 2024**: This project uses Rust edition 2024 (unstable), requires nightly
- **Custom lints**: `unnecessary_semicolon`, `declare_interior_mutable_const`, `redundant_clone` are `deny`
- **Build script**: `backend/build.rs` handles proto generation (tonic-build) and copies ONNX/runtime DLLs
- **Debug state**: Only compiled in debug mode; release builds exclude debug features
