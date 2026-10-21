# Frame configuration API

Datum currently exposes frame configuration through Tauri IPC. These commands
use the application's single managed MAVLink connection and do not open a
second serial connection.

## Get supported frame options

Command: `frame_options`

Arguments: none.

The vehicle must be connected and identified as ArduPilot. The response groups
valid selectable frame types by frame class and includes the numeric
`FRAME_CLASS` and `FRAME_TYPE` values used by ArduPilot.

```json
{
  "frame_classes": [
    {
      "id": "quad",
      "label": "Quadcopter",
      "description": "Four-motor multicopter.",
      "parameter_value": 1,
      "types": [
        {
          "id": "x",
          "label": "X",
          "description": "Two motors straddle the forward direction.",
          "parameter_value": 1
        }
      ]
    }
  ],
  "source": "ardupilot_firmware_definition",
  "status": "available"
}
```

PX4 returns an `UNSUPPORTED_FEATURE` error because it uses the airframe
selection parameter `SYS_AUTOSTART` rather than ArduPilot's frame parameters.

## Get the current frame

Command: `current_frame`

Arguments: none.

The backend requests `FRAME_CLASS` and `FRAME_TYPE` from the connected flight
controller and returns only after both actual `PARAM_VALUE` messages arrive.

```json
{
  "frame_class": "quad",
  "frame_type": "x",
  "frame_class_value": 1.0,
  "frame_type_value": 1.0,
  "reboot_required": false
}
```

An unknown value is preserved in its numeric field and has a `null` name. A
read times out after five seconds if either parameter is not returned.

## Set and verify the frame

Command: `set_frame`

Arguments:

```json
{
  "selection": {
    "frame_class": "quad",
    "frame_type": "x"
  }
}
```

The vehicle must be connected, identified as ArduPilot, and disarmed. The
backend rejects unknown frame classes and invalid class/type combinations.
It sends `PARAM_SET` for `FRAME_CLASS` and `FRAME_TYPE`, waits for both
parameter echoes, requests both parameters again, and compares the readback.
It reports success only after the readback matches.

```json
{
  "success": true,
  "verified": true,
  "frame_class": "quad",
  "frame_type": "x",
  "reboot_required": true
}
```

For a class whose firmware ignores `FRAME_TYPE`, such as `tri`, omit
`frame_type`. Any successful change requires a flight-controller reboot before
flight. Frame selection does not change physical motor wiring.

ArduPilot does not define a separate `FRAME_TYPE` value for a generic physical
"Wide X" geometry. Datum therefore does not claim that such an option exists;
the correct mixer must be selected from the firmware-defined layouts.
