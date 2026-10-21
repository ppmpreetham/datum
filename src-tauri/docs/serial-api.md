# Serial connection API

Datum exposes the serial connection API through Tauri IPC. It does not use an
HTTP route for these commands.

## List available ports

Call `list_serial_ports` with no arguments. The command returns an array of:

```json
{
  "portName": "COM3",
  "portType": "usb",
  "usbVendorId": 9900,
  "usbProductId": 17,
  "serialNumber": null,
  "manufacturer": null,
  "product": "Legacy FMU"
}
```

## Connect to a selected port

Call `connect_serial_vehicle` with:

```json
{
  "portName": "COM3",
  "baudRate": 115200
}
```

The command validates the port name and baud rate, opens the MAVLink serial
connection, and returns the current vehicle status. Vehicle state changes are
also emitted through the `datum://vehicle` Tauri event.

Supported baud rates are `9600`, `19200`, `38400`, `57600`, `115200`,
`230400`, `460800`, `500000`, `576000`, `921600`, and `1500000`.

