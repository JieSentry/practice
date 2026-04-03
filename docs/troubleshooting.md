# Troubleshooting

- [Wrong Map Detection](#wrong-map-detection)
- [Actions Contention](#actions-contention)
- [Default Ratio Game Resolution](#default-ratio-game-resolution)
- [Preventing Double Jumps](#preventing-double-jumps)
- [Up Jump Key](#up-jump-key)
- [Missing Installation](#missing-installation)
- [Unstucking State](#unstucking-state)

---

## Wrong Map Detection

Map detection may fail or select the wrong map under the following conditions:

- Rapidly moving between different maps  
- Other UI elements overlapping the minimap  
- The minimap is not fully expanded  

> **Rule of Thumb:**  
> The bot always attempts to crop the **white minimap border** as tightly as possible.  
> Before creating a new map, ensure the displayed minimap looks clean and closely matches the in-game version with the white border properly cropped.

### Fix Methods

- Below the minimap, there is a `Re-detect` button that can be used to force the bot to re-detect the current map.  
- Move or reposition the minimap window.  

## Actions Contention

`Every milliseconds` actions can sometimes lead to contention if their intervals are too close.

For example, if you have two `Every milliseconds` actions running every 2 seconds, each followed by a short wait (e.g., 1 second), and a normal action in between, the normal action may never get a chance to execute properly.

This situation is uncommon but can occur when multiple timed actions overlap too tightly.

## Default Ratio Game Resolution

Currently, `Default Ratio` game resolution is not supported because most detection templates are built for `Ideal Ratio` resolutions:

- `1920x1080` - Requires choosing `Ideal Ratio` through in-game settings.
- `1367x768` or below - This appears like `Ideal Ratio` even without setting it explicitly.

Using `Default Ratio` at `1920x1080` or higher causes the UI to appear blurry, making the bot fail to detect.

## Preventing Double Jumps

> **Note:** This behavior is subject to change in future versions.

If you want the bot to `walk instead of double jump` between two points, ensure that the horizontal distance (`x`) between them is `less than 25 units`.  
At that distance, the bot considers the destination `close enough` to walk.

## Up Jump Key

The `Up jump key` is optional and only required for classes that have a dedicated up jump skill.  
Here are some general ideas on how to configure it and more specific cases can be referenced below:

- If your class performs an up jump by holding the `Up arrow + Jump`, you do not need to set a separate up jump key.  
- If your class has a dedicated up jump skill, assign that skill as the `Up jump key`.  

### Mage Classes

- Set your `Teleport key`.  
- The bot will use one of the following combinations where appropriate:
  - Teleport only  
  - Jump → Teleport  
  - Up jump → Teleport

### Classes Using Up arrow

- Set the `Up arrow` as the `Up jump key`.

### Classes Using Up Jump Skill

- If your up jump is short but usable mid-air (e.g., Night Lord), enable `Jump then up jump if possible`.

### Flight-Based Classes (e.g., Illium, Hoyoung)

- Enable `Up jump is flight`.

## Missing Installation

If the bot fails to start or shows missing component errors on a fresh Windows installation, ensure the following dependencies are installed:

- [**Visual C++ Redistributable 2015–2022**](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist#visual-studio-2015-2017-2019-and-2022)  
- [**Microsoft WebView2 Runtime**](https://developer.microsoft.com/en-us/microsoft-edge/webview2?form=MA13LH)
- CUDA 12.x is required to enable GPU usage.

## Unstucking State

The `Unstucking` state helps the bot recover when stuck due to dialogs, ropes, or undetectable player positions.  
However, it can also trigger incorrectly if setup issues occur.

### Common Causes

- The bot detects the minimap successfully but fails to detect the player, assuming the player is stuck at map edges.  
- The bot attempts a movement action, but the player does not move within after a while.  
- When using remote control setup, the following can occur:
  - The `Num lock key` can cause incorrect key sending (e.g., sending `4826` instead of arrow keys) in `Default Input Method`. 
  - Running the bot remotely requires proper setup. Check the [remote control documentation](https://github.com/sasanquaa/komari/blob/master/docs/remote_control.md) for more details.
