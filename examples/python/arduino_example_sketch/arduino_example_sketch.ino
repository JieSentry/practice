#include <Keyboard.h>
#include <Mouse.h>

enum class Command {
  KEY = 0,
  KEY_DOWN = 1,
  KEY_UP = 2,
  MOUSE_MOVE = 3,
  MOUSE_CLICK = 4,
  MOUSE_SCROLL = 5
};

const size_t CMD_KEY_ARGS_COUNT = 1;
const size_t CMD_MOUSE_MOVE_ARGS_COUNT = 4;
const size_t CMD_MOUSE_SCROLL_ARGS_COUNT = 2;
const size_t MAX_ARGS_COUNT = max(max(CMD_KEY_ARGS_COUNT, CMD_MOUSE_MOVE_ARGS_COUNT), CMD_MOUSE_SCROLL_ARGS_COUNT);

uint8_t ARGS[MAX_ARGS_COUNT];

void setup() {
  Serial.begin(9600);
  Keyboard.begin();
  Mouse.begin();
}

// NOTE: This logics are vibe-coded with some provision. Not fully tested but seems to running fine.
void loop() {
  if (Serial.available()) {
    int cmd = Serial.read();
    if (cmd < static_cast<int>(Command::KEY) || cmd > static_cast<int>(Command::MOUSE_SCROLL)) {
      return;
    }

    Command command = static_cast<Command>(cmd);
    size_t args_count = getArgsCount(command);
    size_t read_count = Serial.readBytes(ARGS, args_count);
    if (read_count != args_count) {
      return;
    }

    switch (command) {
      case Command::KEY:
        {
          Keyboard.write(ARGS[0]);
          break;
        }

      case Command::KEY_DOWN:
        {
          Keyboard.press(ARGS[0]);
          break;
        }

      case Command::KEY_UP:
        {
          Keyboard.release(ARGS[0]);
          break;
        }

      case Command::MOUSE_MOVE:
        {
          int16_t dx = getInt16(ARGS[0], ARGS[1]);
          int16_t dy = getInt16(ARGS[2], ARGS[3]);
          while (dx != 0 || dy != 0) {
            int8_t step_x = constrain(dx, -128, 127);
            int8_t step_y = constrain(dy, -128, 127);
            Mouse.move(step_x, step_y);
            dx -= step_x;
            dy -= step_y;
          }
          break;
        }

      case Command::MOUSE_CLICK:
        {
          Mouse.click(MOUSE_LEFT);
          break;
        }

      case Command::MOUSE_SCROLL:
        {
          Mouse.move(0, 0, getInt16(ARGS[0], ARGS[1]));  // z-axis = scroll
          break;
        }

      default:
        break;
    }
  }
}

/*
  Converts two uint8_t to int16_t.

  `lo` represents the least signification byte and `hi` represents the most signification byte.
*/
int16_t getInt16(uint8_t lo, uint8_t hi) {
  return static_cast<int16_t>(lo | (hi << 8));
}

/*
  Gets how many extra bytes each command needs as arguments.
*/
size_t getArgsCount(Command command) {
  switch (command) {
    case Command::KEY:
    case Command::KEY_DOWN:
    case Command::KEY_UP:
      return CMD_KEY_ARGS_COUNT;
    case Command::MOUSE_SCROLL:
      return CMD_MOUSE_SCROLL_ARGS_COUNT;
    case Command::MOUSE_MOVE:
      return CMD_MOUSE_MOVE_ARGS_COUNT;
    case Command::MOUSE_CLICK:
      return 0;
    default:
      return 0;
  }
}
