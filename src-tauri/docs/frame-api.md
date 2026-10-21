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

ArduPilot does not define a separate `FRAME_TYPE` value for a generic physical
"Wide X" geometry. Datum therefore does not claim that such an option exists;
the correct mixer must be selected from the firmware-defined layouts.
