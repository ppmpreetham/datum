use mavlink::{ardupilotmega as mav, MavConnection, MavHeader};
use serde::{Deserialize, Serialize};
use serialport::{SerialPortInfo, SerialPortType};
use std::{
    sync::{mpsc, Mutex},
    thread,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Firmware {
    ArduPilot,
    Px4,
}

impl Firmware {
    fn from_mavlink(value: u8) -> Result<Self, String> {
        match value {
            3 => Ok(Self::ArduPilot),
            12 => Ok(Self::Px4),
            _ => Err(format!("Unsupported flight controller firmware (MAVLink autopilot {value}). Datum supports ArduPilot and PX4.")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationKind {
    Gyro,
    Accelerometer,
    Compass,
    Level,
    Radio,
    Esc,
    MotorTest,
}

impl CalibrationKind {
    fn command(self, firmware: Firmware) -> Result<[f32; 7], String> {
        let mut params = [0.0; 7];
        match self {
            Self::Gyro => params[0] = 1.0,
            Self::Accelerometer => params[4] = 1.0,
            Self::Compass if firmware == Firmware::Px4 => params[1] = 1.0,
            Self::Compass => params[2] = 1.0,
            Self::Level => params[4] = 2.0,
            Self::Radio if firmware == Firmware::ArduPilot => {
                return Err("ArduPilot does not calibrate radio endpoints through MAV_CMD_PREFLIGHT_CALIBRATION. Use the flight controller's radio setup procedure.".into())
            }
            Self::Radio => params[3] = 1.0,
            Self::Esc | Self::MotorTest => {
                return Err(
                    "Motor-running operations are disabled until validated with real hardware."
                        .into(),
                )
            }
        }
        Ok(params)
    }

    fn command_id(self, firmware: Firmware) -> mav::MavCmd {
        if self == Self::Compass && firmware == Firmware::ArduPilot {
            mav::MavCmd::MAV_CMD_DO_START_MAG_CAL
        } else {
            mav::MavCmd::MAV_CMD_PREFLIGHT_CALIBRATION
        }
    }
}

fn position_prompt(position: f32) -> Option<(&'static str, u8)> {
    match position as u32 {
        1 => Some(("Place the drone level, then press Continue", 0)),
        2 => Some(("Place the drone on its left side, then press Continue", 17)),
        3 => Some(("Place the drone on its right side, then press Continue", 34)),
        4 => Some(("Place the drone nose down, then press Continue", 50)),
        5 => Some(("Place the drone nose up, then press Continue", 67)),
        6 => Some(("Place the drone upside down, then press Continue", 84)),
        _ => None,
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Disconnected,
    Connecting,
    Connected,
    Unsupported,
    Error,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationPhase {
    Starting,
    Accepted,
    InProgress,
    AwaitingUser,
    Stopping,
    Stopped,
    Completed,
    Failed,
    Unknown,
}

#[derive(Clone, Debug, Serialize)]
pub struct CalibrationProgress {
    pub kind: CalibrationKind,
    pub phase: CalibrationPhase,
    pub percent: Option<u8>,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SensorCondition {
    Unknown,
    Healthy,
    NeedsAttention,
}

#[derive(Clone, Debug, Serialize)]
pub struct SensorReading {
    pub kind: CalibrationKind,
    pub condition: SensorCondition,
}

fn sensor_readings(present: u32, healthy: u32) -> Vec<SensorReading> {
    [
        (CalibrationKind::Gyro, 1),
        (CalibrationKind::Accelerometer, 2),
        (CalibrationKind::Compass, 4),
        (CalibrationKind::Radio, 65536),
    ]
    .into_iter()
    .map(|(kind, bit)| SensorReading {
        kind,
        condition: if present & bit == 0 {
            SensorCondition::Unknown
        } else if healthy & bit == 0 {
            SensorCondition::NeedsAttention
        } else {
            SensorCondition::Healthy
        },
    })
    .collect()
}

#[derive(Clone, Debug, Serialize)]
pub struct CalibrationOption {
    pub kind: CalibrationKind,
    pub available: bool,
    pub instructions: &'static str,
    pub note: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FrameTypeOption {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub parameter_value: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FrameClassOption {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub parameter_value: u8,
    pub types: Vec<FrameTypeOption>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FrameOptionsResponse {
    pub frame_classes: Vec<FrameClassOption>,
    pub source: &'static str,
    pub status: &'static str,
}

fn frame_type(
    id: &'static str,
    label: &'static str,
    description: &'static str,
    parameter_value: u8,
) -> FrameTypeOption {
    FrameTypeOption {
        id,
        label,
        description,
        parameter_value,
    }
}

fn ardupilot_frame_options() -> FrameOptionsResponse {
    let common = || {
        vec![
            frame_type("plus", "Plus", "One motor points forward.", 0),
            frame_type("x", "X", "Two motors straddle the forward direction.", 1),
            frame_type("v", "V", "V-shaped motor layout.", 2),
            frame_type("h", "H", "H-shaped motor layout.", 3),
        ]
    };
    FrameOptionsResponse {
        frame_classes: vec![
            FrameClassOption {
                id: "quad",
                label: "Quadcopter",
                description: "Four-motor multicopter.",
                parameter_value: 1,
                types: vec![
                    frame_type("plus", "Plus", "One motor points forward.", 0),
                    frame_type("x", "X", "Two motors straddle the forward direction.", 1),
                    frame_type("v", "V", "V-shaped quad layout.", 2),
                    frame_type("h", "H", "H-shaped quad layout.", 3),
                    frame_type("v_tail", "V-Tail", "Quad with a V-tail motor layout.", 4),
                    frame_type("a_tail", "A-Tail", "Quad with an A-tail motor layout.", 5),
                    frame_type(
                        "betaflight_x",
                        "BetaFlight X",
                        "X geometry using BetaFlight motor ordering.",
                        12,
                    ),
                    frame_type("dji_x", "DJI X", "X geometry using DJI motor ordering.", 13),
                    frame_type(
                        "clockwise_x",
                        "Clockwise X",
                        "X geometry using clockwise motor ordering.",
                        14,
                    ),
                    frame_type(
                        "betaflight_x_reversed",
                        "BetaFlight X Reversed",
                        "BetaFlight X ordering with reversed motor directions.",
                        18,
                    ),
                    frame_type("y4", "Y4", "Four motors arranged in a Y layout.", 19),
                ],
            },
            FrameClassOption {
                id: "hexa",
                label: "Hexacopter",
                description: "Six-motor multicopter.",
                parameter_value: 2,
                types: common(),
            },
            FrameClassOption {
                id: "octa",
                label: "Octocopter",
                description: "Eight-motor single-plane multicopter.",
                parameter_value: 3,
                types: vec![
                    frame_type("plus", "Plus", "One motor points forward.", 0),
                    frame_type("x", "X", "Two motors straddle the forward direction.", 1),
                    frame_type("v", "V", "V-shaped motor layout.", 2),
                    frame_type("h", "H", "H-shaped motor layout.", 3),
                    frame_type("i", "I", "Sideways H octocopter layout.", 15),
                ],
            },
            FrameClassOption {
                id: "octa_quad",
                label: "OctaQuad",
                description: "Eight motors in four coaxial pairs.",
                parameter_value: 4,
                types: common(),
            },
            FrameClassOption {
                id: "y6",
                label: "Y6",
                description: "Six motors in three coaxial pairs.",
                parameter_value: 5,
                types: vec![
                    frame_type("y6b", "Y6B", "ArduPilot Y6B motor arrangement.", 10),
                    frame_type("y6f", "Y6F", "FireFly Y6 motor arrangement.", 11),
                ],
            },
            FrameClassOption {
                id: "tri",
                label: "Tricopter",
                description: "Three-motor frame; FRAME_TYPE is ignored by ArduPilot.",
                parameter_value: 7,
                types: vec![],
            },
            FrameClassOption {
                id: "dodeca_hexa",
                label: "DodecaHexa",
                description: "Twelve motors in six coaxial pairs.",
                parameter_value: 12,
                types: common(),
            },
            FrameClassOption {
                id: "deca",
                label: "Decacopter",
                description: "Ten-motor multicopter.",
                parameter_value: 14,
                types: common(),
            },
        ],
        source: "ardupilot_firmware_definition",
        status: "available",
    }
}

#[tauri::command]
pub fn frame_options(
    service: State<'_, CalibrationService>,
) -> Result<FrameOptionsResponse, String> {
    let session = service.0.lock().unwrap();
    match session.status.firmware {
        Some(Firmware::ArduPilot) => Ok(ardupilot_frame_options()),
        Some(Firmware::Px4) => Err("UNSUPPORTED_FEATURE: PX4 airframes use SYS_AUTOSTART; ArduPilot FRAME_CLASS and FRAME_TYPE values cannot be applied.".into()),
        None => Err("Connect a supported flight controller before requesting frame options.".into()),
    }
}

#[tauri::command]
pub fn calibration_options() -> Vec<CalibrationOption> {
    vec![
        CalibrationOption { kind: CalibrationKind::Gyro, available: true,
            instructions: "Place the drone still on a level surface and follow flight controller prompts.",
            note: "The controller may reject this procedure if it is unavailable." },
        CalibrationOption { kind: CalibrationKind::Accelerometer, available: true,
            instructions: "Remove propellers. Place the drone in each orientation requested by the controller.",
            note: "The controller may reject this procedure if it is unavailable." },
        CalibrationOption { kind: CalibrationKind::Compass, available: true,
            instructions: "Move away from metal and magnets. Rotate the drone as directed by the controller.",
            note: "The controller may reject this procedure if it is unavailable." },
        CalibrationOption { kind: CalibrationKind::Level, available: true,
            instructions: "Place the drone on a level, motionless surface.",
            note: "The controller may reject this procedure if it is unavailable." },
        CalibrationOption { kind: CalibrationKind::Radio, available: true,
            instructions: "Turn on the transmitter and follow the controller prompts to move all sticks and switches.",
            note: "Available through this command on PX4. ArduPilot radio endpoints require its separate setup procedure." },
        CalibrationOption { kind: CalibrationKind::Esc, available: false,
            instructions: "", note: "Disabled until verified with real hardware." },
        CalibrationOption { kind: CalibrationKind::MotorTest, available: false,
            instructions: "", note: "Disabled until verified with real hardware." },
    ]
}

#[derive(Clone, Debug, Serialize)]
pub struct VehicleStatus {
    pub phase: Phase,
    pub address: Option<String>,
    pub firmware: Option<Firmware>,
    pub system_id: Option<u8>,
    pub component_id: Option<u8>,
    pub armed: Option<bool>,
    pub message: String,
    pub calibration: Option<CalibrationProgress>,
    pub sensors: Vec<SensorReading>,
}

impl Default for VehicleStatus {
    fn default() -> Self {
        Self {
            phase: Phase::Disconnected,
            address: None,
            firmware: None,
            system_id: None,
            component_id: None,
            armed: None,
            message: "No vehicle connected".into(),
            calibration: None,
            sensors: Vec::new(),
        }
    }
}

enum WorkerCommand {
    Start(CalibrationKind),
    Continue,
    Stop,
    Disconnect,
}

#[derive(Default)]
struct Session {
    status: VehicleStatus,
    sender: Option<mpsc::Sender<WorkerCommand>>,
    generation: u64,
}

#[derive(Default)]
pub struct CalibrationService(Mutex<Session>);

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SerialPortSummary {
    pub port_name: String,
    pub port_type: &'static str,
    pub usb_vendor_id: Option<u16>,
    pub usb_product_id: Option<u16>,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub serial_number: Option<String>,
}

fn port_summary(port: SerialPortInfo) -> SerialPortSummary {
    let (port_type, usb_vendor_id, usb_product_id, manufacturer, product, serial_number) =
        match port.port_type {
            SerialPortType::UsbPort(info) => (
                "usb",
                Some(info.vid),
                Some(info.pid),
                info.manufacturer,
                info.product,
                info.serial_number,
            ),
            SerialPortType::BluetoothPort => ("bluetooth", None, None, None, None, None),
            SerialPortType::PciPort => ("pci", None, None, None, None, None),
            SerialPortType::Unknown => ("unknown", None, None, None, None, None),
        };
    SerialPortSummary {
        port_name: port.port_name,
        port_type,
        usb_vendor_id,
        usb_product_id,
        manufacturer,
        product,
        serial_number,
    }
}

#[tauri::command]
pub fn list_serial_ports() -> Result<Vec<SerialPortSummary>, String> {
    let mut ports = serialport::available_ports()
        .map_err(|error| format!("Could not list Windows serial ports: {error}"))?
        .into_iter()
        .map(port_summary)
        .collect::<Vec<_>>();
    ports.sort_by(|left, right| {
        (left.port_type != "usb", &left.port_name)
            .cmp(&(right.port_type != "usb", &right.port_name))
    });
    Ok(ports)
}

fn validate_address(address: &str) -> Result<(), String> {
    let parts: Vec<&str> = address.split(':').collect();
    if parts.len() != 3 {
        return Err("Enter a MAVLink address such as serial:COM3:115200, udpin:127.0.0.1:14550, or tcpout:127.0.0.1:5760.".into());
    }
    match parts[0] {
        "serial" if !parts[1].is_empty() && parts[2].parse::<u32>().is_ok() => Ok(()),
        "udpin" if matches!(parts[1], "127.0.0.1" | "localhost") && parts[2].parse::<u16>().is_ok() => Ok(()),
        "udpout" | "tcpout" if !parts[1].is_empty() && parts[2].parse::<u16>().is_ok() => Ok(()),
        _ => Err("Unsupported connection address. Use USB serial, local UDP listener, UDP client, or TCP client.".into()),
    }
}

fn change(
    app: &AppHandle,
    service: &CalibrationService,
    generation: u64,
    f: impl FnOnce(&mut Session),
) {
    let snapshot = {
        let mut session = service.0.lock().unwrap();
        if session.generation != generation {
            return;
        }
        f(&mut session);
        session.status.clone()
    };
    let _ = app.emit("datum://vehicle", snapshot);
}

#[tauri::command]
pub fn vehicle_status(service: State<'_, CalibrationService>) -> VehicleStatus {
    service.0.lock().unwrap().status.clone()
}

#[tauri::command]
pub fn connect_vehicle(
    address: String,
    app: AppHandle,
    service: State<'_, CalibrationService>,
) -> Result<VehicleStatus, String> {
    validate_address(&address)?;
    let (sender, receiver) = mpsc::channel();
    let (generation, snapshot) = {
        let mut session = service.0.lock().unwrap();
        if session.sender.is_some() {
            return Err("A vehicle connection is already active. Disconnect it first.".into());
        }
        session.generation += 1;
        session.status = VehicleStatus {
            phase: Phase::Connecting,
            address: Some(address.clone()),
            message: "Waiting for a flight controller heartbeat".into(),
            ..VehicleStatus::default()
        };
        session.sender = Some(sender);
        (session.generation, session.status.clone())
    };
    let _ = app.emit("datum://vehicle", snapshot.clone());
    thread::spawn(move || run_vehicle(app, generation, address, receiver));
    Ok(snapshot)
}

#[tauri::command]
pub fn connect_serial_vehicle(
    port_name: String,
    baud_rate: u32,
    app: AppHandle,
    service: State<'_, CalibrationService>,
) -> Result<VehicleStatus, String> {
    if baud_rate == 0 {
        return Err("Select a baud rate greater than zero.".into());
    }
    let ports = list_serial_ports()?;
    if !ports.iter().any(|port| port.port_name == port_name) {
        return Err(format!(
            "{port_name} is not an available serial port. Refresh the port list."
        ));
    }
    connect_vehicle(format!("serial:{port_name}:{baud_rate}"), app, service)
}

#[tauri::command]
pub fn disconnect_vehicle(app: AppHandle, service: State<'_, CalibrationService>) -> VehicleStatus {
    let snapshot = {
        let mut session = service.0.lock().unwrap();
        if let Some(sender) = session.sender.take() {
            let _ = sender.send(WorkerCommand::Disconnect);
        }
        session.generation += 1;
        session.status = VehicleStatus::default();
        session.status.clone()
    };
    let _ = app.emit("datum://vehicle", snapshot.clone());
    snapshot
}

#[tauri::command]
pub fn start_calibration(
    kind: CalibrationKind,
    service: State<'_, CalibrationService>,
) -> Result<(), String> {
    let mut session = service.0.lock().unwrap();
    let firmware = session
        .status
        .firmware
        .ok_or("Connect a supported flight controller first.")?;
    if session.status.armed == Some(true) {
        return Err("Disarm the vehicle before calibration.".into());
    }
    if session.status.armed.is_none() {
        return Err("Waiting for vehicle arm state. Try again after a heartbeat.".into());
    }
    if let Some(progress) = &session.status.calibration {
        if matches!(
            progress.phase,
            CalibrationPhase::Starting
                | CalibrationPhase::Accepted
                | CalibrationPhase::InProgress
                | CalibrationPhase::AwaitingUser
                | CalibrationPhase::Stopping
                | CalibrationPhase::Unknown
        ) {
            return Err("Finish or stop the current calibration before starting another.".into());
        }
    }
    kind.command(firmware)?;
    session
        .sender
        .as_ref()
        .ok_or("Vehicle connection is unavailable.")?
        .send(WorkerCommand::Start(kind))
        .map_err(|_| "Vehicle connection was lost.".to_string())?;
    session.status.calibration = Some(CalibrationProgress {
        kind,
        phase: CalibrationPhase::Starting,
        percent: None,
        message: "Preparing calibration request".into(),
    });
    Ok(())
}

#[tauri::command]
pub fn stop_calibration(service: State<'_, CalibrationService>) -> Result<(), String> {
    let session = service.0.lock().unwrap();
    match session.status.calibration.as_ref().map(|c| &c.phase) {
        Some(
            CalibrationPhase::Starting
            | CalibrationPhase::Accepted
            | CalibrationPhase::InProgress
            | CalibrationPhase::AwaitingUser,
        ) => {}
        _ => return Err("No cancellable calibration is active.".into()),
    }
    session
        .sender
        .as_ref()
        .ok_or("Vehicle connection is unavailable.")?
        .send(WorkerCommand::Stop)
        .map_err(|_| "Vehicle connection was lost.".into())
}

#[tauri::command]
pub fn continue_calibration(service: State<'_, CalibrationService>) -> Result<(), String> {
    let session = service.0.lock().unwrap();
    if !matches!(
        session.status.calibration.as_ref().map(|c| &c.phase),
        Some(CalibrationPhase::AwaitingUser)
    ) {
        return Err("The flight controller has not requested the next position.".into());
    }
    session
        .sender
        .as_ref()
        .ok_or("Vehicle connection is unavailable.")?
        .send(WorkerCommand::Continue)
        .map_err(|_| "Vehicle connection was lost.".into())
}

fn run_vehicle(
    app: AppHandle,
    generation: u64,
    address: String,
    commands: mpsc::Receiver<WorkerCommand>,
) {
    let service = app.state::<CalibrationService>();
    let connection = match mavlink::connect::<mav::MavMessage>(&address) {
        Ok(connection) => connection,
        Err(error) => {
            change(&app, &service, generation, |s| {
                s.status.phase = Phase::Error;
                s.status.message = format!("Could not open vehicle connection: {error}");
                s.sender = None;
            });
            return;
        }
    };
    let mut last_heartbeat: Option<Instant> = None;
    let started = Instant::now();
    let mut sequence = 0u8;
    let mut pending: Option<(CalibrationKind, mav::MavCmd, Instant)> = None;
    let mut stopping: Option<Instant> = None;
    let mut compass_expected = 0u8;
    let mut compass_success = 0u8;
    let mut awaiting_position: Option<f32> = None;
    let mut last_gcs_heartbeat = Instant::now() - Duration::from_secs(2);
    loop {
        if last_gcs_heartbeat.elapsed() >= Duration::from_secs(1) {
            sequence = sequence.wrapping_add(1);
            let header = MavHeader {
                system_id: 255,
                component_id: 190,
                sequence,
            };
            let heartbeat = mav::MavMessage::HEARTBEAT(mav::HEARTBEAT_DATA {
                custom_mode: 0,
                mavtype: mav::MavType::MAV_TYPE_GCS,
                autopilot: mav::MavAutopilot::MAV_AUTOPILOT_INVALID,
                base_mode: mav::MavModeFlag::empty(),
                system_status: mav::MavState::MAV_STATE_ACTIVE,
                mavlink_version: 3,
            });
            let _ = connection.send(&header, &heartbeat);
            last_gcs_heartbeat = Instant::now();
        }
        while let Ok(command) = commands.try_recv() {
            match command {
                WorkerCommand::Disconnect => return,
                WorkerCommand::Continue => {
                    if let Some(position) = awaiting_position {
                        let status = service.0.lock().unwrap().status.clone();
                        if let (Some(system_id), Some(component_id)) =
                            (status.system_id, status.component_id)
                        {
                            sequence = sequence.wrapping_add(1);
                            let header = MavHeader {
                                system_id: 255,
                                component_id: 190,
                                sequence,
                            };
                            let data = mav::COMMAND_LONG_DATA {
                                target_system: system_id,
                                target_component: component_id,
                                command: mav::MavCmd::MAV_CMD_ACCELCAL_VEHICLE_POS,
                                confirmation: 0,
                                param1: position,
                                param2: 0.0,
                                param3: 0.0,
                                param4: 0.0,
                                param5: 0.0,
                                param6: 0.0,
                                param7: 0.0,
                            };
                            if connection
                                .send(&header, &mav::MavMessage::COMMAND_LONG(data))
                                .is_ok()
                            {
                                awaiting_position = None;
                                change(&app, &service, generation, |s| {
                                    if let Some(progress) = &mut s.status.calibration {
                                        progress.phase = CalibrationPhase::InProgress;
                                        progress.message =
                                            "Position sent; waiting for the next instruction"
                                                .into();
                                    }
                                });
                            }
                        }
                    }
                }
                WorkerCommand::Start(kind) => {
                    let status = service.0.lock().unwrap().status.clone();
                    let (Some(firmware), Some(system_id), Some(component_id)) =
                        (status.firmware, status.system_id, status.component_id)
                    else {
                        continue;
                    };
                    let params = match kind.command(firmware) {
                        Ok(params) => params,
                        Err(_) => continue,
                    };
                    let command_id = kind.command_id(firmware);
                    let data = mav::COMMAND_LONG_DATA {
                        target_system: system_id,
                        target_component: component_id,
                        command: command_id,
                        confirmation: 0,
                        param1: params[0],
                        param2: params[1],
                        param3: params[2],
                        param4: params[3],
                        param5: params[4],
                        param6: params[5],
                        param7: params[6],
                    };
                    if kind == CalibrationKind::Compass && firmware == Firmware::ArduPilot {
                        for message_id in [191.0, 192.0] {
                            sequence = sequence.wrapping_add(1);
                            let stream_header = MavHeader {
                                system_id: 255,
                                component_id: 190,
                                sequence,
                            };
                            let request = mav::COMMAND_LONG_DATA {
                                target_system: system_id,
                                target_component: component_id,
                                command: mav::MavCmd::MAV_CMD_SET_MESSAGE_INTERVAL,
                                confirmation: 0,
                                param1: message_id,
                                param2: 500_000.0,
                                param3: 0.0,
                                param4: 0.0,
                                param5: 0.0,
                                param6: 0.0,
                                param7: 0.0,
                            };
                            if let Err(error) = connection
                                .send(&stream_header, &mav::MavMessage::COMMAND_LONG(request))
                            {
                                change(&app, &service, generation, |s| {
                                    s.status.message =
                                        format!("Could not request compass progress: {error}")
                                });
                            }
                        }
                    }
                    sequence = sequence.wrapping_add(1);
                    let header = MavHeader {
                        system_id: 255,
                        component_id: 190,
                        sequence,
                    };
                    match connection.send(&header, &mav::MavMessage::COMMAND_LONG(data)) {
                        Ok(_) => {
                            pending = Some((kind, command_id, Instant::now()));
                            compass_expected = 0;
                            compass_success = 0;
                            awaiting_position = None;
                            change(&app, &service, generation, |s| {
                                s.status.calibration = Some(CalibrationProgress {
                                kind, phase: CalibrationPhase::Starting, percent: None,
                                message: "Calibration request sent; waiting for the flight controller".into(),
                            })
                            });
                        }
                        Err(error) => change(&app, &service, generation, |s| {
                            s.status.message =
                                format!("Could not send calibration request: {error}")
                        }),
                    }
                }
                WorkerCommand::Stop => {
                    if let Some((kind, command_id, _)) = pending {
                        let status = service.0.lock().unwrap().status.clone();
                        if let (Some(system_id), Some(component_id)) =
                            (status.system_id, status.component_id)
                        {
                            sequence = sequence.wrapping_add(1);
                            let header = MavHeader {
                                system_id: 255,
                                component_id: 190,
                                sequence,
                            };
                            let cancel = mav::COMMAND_CANCEL_DATA {
                                target_system: system_id,
                                target_component: component_id,
                                command: command_id,
                            };
                            match connection.send(&header, &mav::MavMessage::COMMAND_CANCEL(cancel))
                            {
                                Ok(_) => {
                                    stopping = Some(Instant::now());
                                    change(&app, &service, generation, |s| {
                                        s.status.calibration = Some(CalibrationProgress {
                                        kind, phase: CalibrationPhase::Stopping, percent: None,
                                        message: "Stop requested; waiting for flight controller confirmation".into(),
                                    })
                                    });
                                }
                                Err(error) => change(&app, &service, generation, |s| {
                                    s.status.message = format!("Cannot cancel calibration: {error}")
                                }),
                            }
                        }
                    }
                }
            }
        }
        if let Ok((header, message)) = connection.try_recv() {
            let selected_system = service.0.lock().unwrap().status.system_id;
            if selected_system.is_some_and(|id| id != header.system_id) {
                continue;
            }
            match message {
                mav::MavMessage::HEARTBEAT(heartbeat)
                    if heartbeat.autopilot != mav::MavAutopilot::MAV_AUTOPILOT_INVALID =>
                {
                    let firmware = Firmware::from_mavlink(heartbeat.autopilot as u8);
                    last_heartbeat = Some(Instant::now());
                    change(&app, &service, generation, |s| {
                        if let Some(existing_id) = s.status.system_id {
                            if existing_id != header.system_id {
                                return;
                            }
                        }
                        s.status.system_id = Some(header.system_id);
                        s.status.component_id = Some(header.component_id);
                        s.status.armed = Some(
                            heartbeat
                                .base_mode
                                .contains(mav::MavModeFlag::MAV_MODE_FLAG_SAFETY_ARMED),
                        );
                        match firmware {
                            Ok(firmware) => {
                                s.status.phase = Phase::Connected;
                                s.status.firmware = Some(firmware);
                                s.status.message = "Flight controller connected".into();
                            }
                            Err(error) => {
                                s.status.phase = Phase::Unsupported;
                                s.status.message = error;
                            }
                        }
                    });
                }
                mav::MavMessage::COMMAND_ACK(ack) => {
                    if let Some((kind, command_id, _)) = pending {
                        if ack.command == command_id {
                            if let Some((_, _, last_activity)) = &mut pending {
                                *last_activity = Instant::now();
                            }
                            let current_progress =
                                service.0.lock().unwrap().status.calibration.clone();
                            let prior_in_progress = matches!(
                                current_progress.as_ref().map(|c| &c.phase),
                                Some(CalibrationPhase::InProgress)
                            );
                            let awaiting_user = awaiting_position.is_some();
                            let ardupilot_compass = kind == CalibrationKind::Compass
                                && service.0.lock().unwrap().status.firmware
                                    == Some(Firmware::ArduPilot);
                            let (phase, text) = match ack.result {
                                mav::MavResult::MAV_RESULT_IN_PROGRESS => {
                                    (CalibrationPhase::InProgress, "Calibration in progress")
                                }
                                mav::MavResult::MAV_RESULT_CANCELLED => (
                                    CalibrationPhase::Stopped,
                                    "Calibration stopped by flight controller",
                                ),
                                mav::MavResult::MAV_RESULT_ACCEPTED if stopping.is_some() => (
                                    CalibrationPhase::Unknown,
                                    "Stop was not confirmed; check the flight controller",
                                ),
                                mav::MavResult::MAV_RESULT_ACCEPTED if awaiting_user => (
                                    CalibrationPhase::AwaitingUser,
                                    "Place the drone as instructed, then press Continue",
                                ),
                                mav::MavResult::MAV_RESULT_ACCEPTED
                                    if prior_in_progress && ardupilot_compass =>
                                {
                                    (
                                        CalibrationPhase::InProgress,
                                        "Compass calibration in progress; waiting for reports",
                                    )
                                }
                                mav::MavResult::MAV_RESULT_ACCEPTED if prior_in_progress => (
                                    CalibrationPhase::Completed,
                                    "Calibration completed according to the flight controller",
                                ),
                                mav::MavResult::MAV_RESULT_ACCEPTED => (
                                    CalibrationPhase::Accepted,
                                    "Request accepted; waiting for completion confirmation",
                                ),
                                _ => (
                                    CalibrationPhase::Failed,
                                    "Flight controller rejected or failed the calibration",
                                ),
                            };
                            if matches!(
                                phase,
                                CalibrationPhase::Failed
                                    | CalibrationPhase::Stopped
                                    | CalibrationPhase::Completed
                            ) {
                                pending = None;
                                stopping = None;
                            }
                            let percent = if matches!(
                                phase,
                                CalibrationPhase::InProgress | CalibrationPhase::AwaitingUser
                            ) {
                                current_progress
                                    .as_ref()
                                    .and_then(|progress| progress.percent)
                            } else if matches!(phase, CalibrationPhase::Completed) {
                                Some(100)
                            } else {
                                None
                            };
                            let message = if matches!(phase, CalibrationPhase::AwaitingUser) {
                                current_progress
                                    .map(|progress| progress.message)
                                    .unwrap_or_else(|| text.into())
                            } else {
                                text.into()
                            };
                            change(&app, &service, generation, |s| {
                                s.status.calibration = Some(CalibrationProgress {
                                    kind,
                                    phase,
                                    percent,
                                    message,
                                })
                            });
                        }
                    }
                }
                mav::MavMessage::COMMAND_LONG(request)
                    if request.command == mav::MavCmd::MAV_CMD_ACCELCAL_VEHICLE_POS =>
                {
                    if let Some((CalibrationKind::Accelerometer, _, _)) = pending {
                        if let Some((prompt, percent)) = position_prompt(request.param1) {
                            awaiting_position = Some(request.param1);
                            if let Some((_, _, last_activity)) = &mut pending {
                                *last_activity = Instant::now();
                            }
                            change(&app, &service, generation, |s| {
                                s.status.calibration = Some(CalibrationProgress {
                                    kind: CalibrationKind::Accelerometer,
                                    phase: CalibrationPhase::AwaitingUser,
                                    percent: Some(percent),
                                    message: prompt.into(),
                                })
                            });
                        } else if request.param1 as u32 == 16777215 {
                            pending = None;
                            change(&app, &service, generation, |s| {
                                s.status.calibration = Some(CalibrationProgress {
                                kind: CalibrationKind::Accelerometer, phase: CalibrationPhase::Completed,
                                percent: Some(100), message: "Accelerometer calibration completed according to the flight controller".into(),
                            })
                            });
                        } else if request.param1 as u32 == 16777216 {
                            pending = None;
                            change(&app, &service, generation, |s| {
                                s.status.calibration = Some(CalibrationProgress {
                                    kind: CalibrationKind::Accelerometer,
                                    phase: CalibrationPhase::Failed,
                                    percent: None,
                                    message:
                                        "Accelerometer calibration failed on the flight controller"
                                            .into(),
                                })
                            });
                        }
                    }
                }
                mav::MavMessage::SYS_STATUS(status) => {
                    change(&app, &service, generation, |s| {
                        if s.status.system_id == Some(header.system_id) {
                            s.status.sensors = sensor_readings(
                                status.onboard_control_sensors_present.bits(),
                                status.onboard_control_sensors_health.bits(),
                            );
                        }
                    });
                }
                mav::MavMessage::MAG_CAL_PROGRESS(progress) => {
                    if let Some((CalibrationKind::Compass, _, _)) = pending {
                        if let Some((_, _, last_activity)) = &mut pending {
                            *last_activity = Instant::now();
                        }
                        compass_expected |= progress.cal_mask;
                        change(&app, &service, generation, |s| {
                            s.status.calibration = Some(CalibrationProgress {
                            kind: CalibrationKind::Compass, phase: CalibrationPhase::InProgress,
                            percent: Some(progress.completion_pct), message: "Rotate the drone as instructed until compass calibration finishes".into(),
                        })
                        });
                    }
                }
                mav::MavMessage::MAG_CAL_REPORT(report) => {
                    if let Some((CalibrationKind::Compass, _, _)) = pending {
                        compass_expected |= report.cal_mask;
                        if report.compass_id < 8 {
                            let bit = 1u8 << report.compass_id;
                            if report.cal_status == mav::MagCalStatus::MAG_CAL_SUCCESS
                                && report.autosaved == 1
                            {
                                compass_success |= bit;
                            } else {
                                pending = None;
                                stopping = None;
                                change(&app, &service, generation, |s| {
                                    s.status.calibration = Some(CalibrationProgress {
                                        kind: CalibrationKind::Compass,
                                        phase: CalibrationPhase::Failed,
                                        percent: None,
                                        message: format!(
                                            "Compass {} calibration failed or was not saved",
                                            report.compass_id + 1
                                        ),
                                    })
                                });
                                continue;
                            }
                        }
                        if compass_expected != 0
                            && compass_success & compass_expected == compass_expected
                        {
                            pending = None;
                            stopping = None;
                            change(&app, &service, generation, |s| {
                                s.status.calibration = Some(CalibrationProgress {
                                kind: CalibrationKind::Compass, phase: CalibrationPhase::Completed, percent: Some(100),
                                message: "All reported compasses calibrated and saved. Reboot the vehicle before flight.".into(),
                            })
                            });
                        }
                    }
                }
                mav::MavMessage::STATUSTEXT(status) => {
                    let text = status.text.to_str().unwrap_or_default().to_string();
                    if !text.is_empty() {
                        change(&app, &service, generation, |s| s.status.message = text);
                    }
                }
                _ => {}
            }
        }
        if last_heartbeat.is_none() && started.elapsed() > Duration::from_secs(15) {
            change(&app, &service, generation, |s| {
                s.status.phase = Phase::Error;
                s.status.message = "No flight controller heartbeat received within 15 seconds. Check connection and baud rate.".into();
                s.sender = None;
            });
            return;
        }
        if last_heartbeat.is_some_and(|time| time.elapsed() > Duration::from_secs(5)) {
            change(&app, &service, generation, |s| {
                s.status.phase = Phase::Error;
                s.status.message = "Vehicle heartbeat lost. Check the cable or network.".into();
                if let Some(progress) = &mut s.status.calibration {
                    progress.phase = CalibrationPhase::Unknown;
                    progress.message = "Connection lost; calibration result is unknown".into();
                }
                s.sender = None;
            });
            return;
        }
        if stopping.is_some_and(|time| time.elapsed() > Duration::from_secs(5)) {
            stopping = None;
            change(&app, &service, generation, |s| {
                if let Some(progress) = &mut s.status.calibration {
                    progress.phase = CalibrationPhase::Unknown;
                    progress.message = "Cannot cancel calibration: the flight controller did not confirm the stop request".into();
                }
            });
        }
        if pending.is_some_and(|(_, _, time)| time.elapsed() > Duration::from_secs(60)) {
            pending = None;
            change(&app, &service, generation, |s| {
                if let Some(progress) = &mut s.status.calibration {
                    progress.phase = CalibrationPhase::Unknown;
                    progress.message = "No calibration result received. Check the flight controller before retrying.".into();
                }
            });
        }
        thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serial_port_json_uses_frontend_field_names() {
        let port = SerialPortInfo {
            port_name: "COM3".into(),
            port_type: SerialPortType::UsbPort(serialport::UsbPortInfo {
                vid: 0x26ac,
                pid: 0x0011,
                serial_number: None,
                manufacturer: None,
                product: Some("Legacy FMU".into()),
            }),
        };
        let json = serde_json::to_value(port_summary(port)).unwrap();
        assert_eq!(json["portName"], "COM3");
        assert_eq!(json["portType"], "usb");
        assert_eq!(json["usbVendorId"], 9900);
        assert_eq!(json["usbProductId"], 17);
        assert_eq!(json["product"], "Legacy FMU");
    }

    #[test]
    fn ardupilot_frame_options_keep_firmware_values_distinct() {
        let options = ardupilot_frame_options();
        let quad = options
            .frame_classes
            .iter()
            .find(|class| class.id == "quad")
            .unwrap();
        assert_eq!(quad.parameter_value, 1);
        assert_eq!(
            quad.types
                .iter()
                .find(|kind| kind.id == "x")
                .unwrap()
                .parameter_value,
            1
        );
        assert_eq!(
            quad.types
                .iter()
                .find(|kind| kind.id == "betaflight_x")
                .unwrap()
                .parameter_value,
            12
        );
        assert!(!quad.types.iter().any(|kind| kind.id == "wide_x"));

        let y6 = options
            .frame_classes
            .iter()
            .find(|class| class.id == "y6")
            .unwrap();
        assert_eq!(y6.parameter_value, 5);
        assert_eq!(y6.types.len(), 2);
    }

    #[test]
    fn unsupported_autopilot_is_rejected() {
        assert!(Firmware::from_mavlink(0).is_err());
        assert_eq!(Firmware::from_mavlink(3).unwrap(), Firmware::ArduPilot);
        assert_eq!(Firmware::from_mavlink(12).unwrap(), Firmware::Px4);
    }

    #[test]
    fn motor_operations_are_disabled() {
        assert!(CalibrationKind::Esc.command(Firmware::ArduPilot).is_err());
        assert!(CalibrationKind::MotorTest.command(Firmware::Px4).is_err());
    }

    #[test]
    fn gyro_and_px4_radio_commands_are_distinct() {
        assert_eq!(
            CalibrationKind::Gyro.command(Firmware::Px4).unwrap(),
            [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
        );
        assert_eq!(
            CalibrationKind::Radio.command(Firmware::Px4).unwrap(),
            [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0]
        );
        assert!(CalibrationKind::Radio.command(Firmware::ArduPilot).is_err());
    }

    #[test]
    fn ardupilot_compass_calibration_autosaves_after_success() {
        assert_eq!(
            CalibrationKind::Compass
                .command(Firmware::ArduPilot)
                .unwrap(),
            [0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0]
        );
    }

    #[test]
    fn connection_rejects_remote_bind_and_invalid_addresses() {
        assert!(validate_address("udpin:0.0.0.0:14550").is_err());
        assert!(validate_address("file:recording.tlog").is_err());
        assert!(validate_address("udpin:127.0.0.1:14550").is_ok());
        assert!(validate_address("serial:COM3:115200").is_ok());
    }

    #[test]
    fn unhealthy_reported_sensor_needs_attention() {
        let readings = sensor_readings(0b111, 0b110);
        assert_eq!(readings[0].condition, SensorCondition::NeedsAttention);
        assert_eq!(readings[1].condition, SensorCondition::Healthy);
        assert_eq!(readings[3].condition, SensorCondition::Unknown);
    }

    #[test]
    fn accelerometer_position_requests_have_distinct_prompts() {
        assert_eq!(
            position_prompt(1.0),
            Some(("Place the drone level, then press Continue", 0))
        );
        assert_eq!(
            position_prompt(4.0),
            Some(("Place the drone nose down, then press Continue", 50))
        );
        assert_eq!(position_prompt(9.0), None);
    }
}
