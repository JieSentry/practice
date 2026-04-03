## Customize Input

The bot currently uses the standard Win32 API `SendInput` method for key and mouse input. It does not rely on advanced input method such as `Interception` kernel-level driver.

You can customize the bot to use external input devices/methods (e.g., KMBox, Arduino, etc.) through the `Rpc` method available in the `Settings` tab.  
This approach requires some scripting and setup:

- Use any programming language that supports gRPC stub generation to create and host your input service.  
- Refer to this [example](https://github.com/sasanquaa/komari/tree/master/examples/python) for implementation details:
  - The example assumes a local setup using `http://localhost:5001`.  
  - If the input server runs on the game PC and the bot runs on another PC, update the IP address, configure port forwarding, and ensure the bot can connect to the input server.  
  - You can confirm a successful connection by printing to the console inside the `Init` function.  

![Customize Input](https://github.com/sasanquaa/komari/blob/master/.github/images/customize_input.png?raw=true)

## Key States

Introduced in **v0.20**.

The `KeyState` function is required to let the bot knows if a key is currently pressed or released. This helps improve
some of the bot behaviors. For example, waits for all keys to be released before solving rune. 

```python
def KeyState(self, request: KeyStateRequest, context):
    if is_key_pressed(request.key):
        return KeyStateResponse(KeyState.Pressed)
    else:
        return KeyStateResponse(KeyState.Released)
```

## Random Delay

Introduced in **v0.13**.

- When using the default `SendInput` method, the bot automatically applies a small delay between each key stroke.  
- When using a custom input method via the `Rpc` interface, each `Send` request includes a `down_ms` field — the duration (in milliseconds) that the key should remain pressed.  
  - After sending the key-down event, wait for `down_ms` before sending the key-up event.  
  - This delay is generated using a seed value that is created the first time the bot runs.  
  - If you prefer to manage delays yourself, you can use the `seed` provided in the `Init` request instead — this value is sent each time the bot connects to your input service.

## Mouse

Introduced in **v0.13**.

Mouse input is required for several features, including:

- Auto-revive  
- Familiar swapping  
- HEXA Booster exchange

The mouse behavior depends on the coordinate system used and whether the bot and input server are running on the same PC.  
Two coordinate modes are supported:

- `Relative` – Coordinates `(x, y)` are relative to the game window captured by the bot.  
- `Screen` – Coordinates `(x, y)` are relative to the entire monitor screen containing the game window.  

In your `Init()` function, return the coordinate mode that matches your setup:

```python
def Init(self, request: KeyInitRequest, context):
    return KeyInitResponse(mouse_coordinate=Coordinate.Relative)
```

Mouse input coordinates sent by the bot should be transformed to match your local input system (e.g., KMBox, SendInput):
- If the bot and input server are on the same PC, use `Coordinate.Screen`.
- If they are on different PCs, use `Coordinate.Relative` and apply offsets if needed to account for cropped borders or extra UI elements introduced by remote streaming apps (e.g., Sunshine, Moonlight).

The bot can request the input server to perform the following mouse actions:
- `Move` – Move the cursor to `(x, y)`.
- `Click` – Move to `(x, y)` and perform a click.
- `ScrollDown` – Move to `(x, y)` and scroll down.

Refer to the provided [examples](https://github.com/sasanquaa/komari/tree/master/examples) for implementation details.
