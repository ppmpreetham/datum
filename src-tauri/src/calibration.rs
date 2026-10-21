use mavlink::{ardupilotmega as mav, MavConnection, MavHeader};
use serde::{Deserialize, Serialize};
use serialport::{SerialPortInfo, SerialPortType};
use std::{
    sync::{mpsc, Mutex},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
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

fn position_name(position: f32) -> Option<&'static str> {
    match position as u32 {
        1 => Some("level"),
        2 => Some("left"),
        3 => Some("right"),
        4 => Some("nose_down"),
        5 => Some("nose_up"),
        6 => Some("upside_down"),
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

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FrameConfiguration {
    pub frame_class: Option<String>,
    pub frame_type: Option<String>,
    pub frame_class_value: f32,
    pub frame_type_value: f32,
    pub reboot_required: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct FrameSelection {
    pub frame_class: String,
    pub frame_type: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FrameUpdateResponse {
    pub success: bool,
    pub verified: bool,
    pub frame_class: String,
    pub frame_type: Option<String>,
    pub reboot_required: bool,
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

fn resolve_frame_configuration(class_value: f32, type_value: f32) -> FrameConfiguration {
    let options = ardupilot_frame_options();
    let class = options
        .frame_classes
        .iter()
        .find(|item| item.parameter_value as f32 == class_value);
    let frame_type = class.and_then(|item| {
        item.types
            .iter()
            .find(|kind| kind.parameter_value as f32 == type_value)
    });
    FrameConfiguration {
        frame_class: class.map(|item| item.id.to_string()),
        frame_type: frame_type.map(|item| item.id.to_string()),
        frame_class_value: class_value,
        frame_type_value: type_value,
        reboot_required: false,
    }
}

fn resolve_frame_selection(selection: &FrameSelection) -> Result<(u8, u8), String> {
    let options = ardupilot_frame_options();
    let class = options
        .frame_classes
        .iter()
        .find(|item| item.id == selection.frame_class)
        .ok_or_else(|| format!("INVALID_FRAME_CLASS: {}", selection.frame_class))?;
    if class.types.is_empty() {
        if selection.frame_type.is_some() {
            return Err(format!(
                "INVALID_FRAME_COMBINATION: {} ignores FRAME_TYPE; omit frame_type.",
                selection.frame_class
            ));
        }
        return Ok((class.parameter_value, 0));
    }
    let requested_type = selection
        .frame_type
        .as_deref()
        .ok_or("INVALID_FRAME_TYPE: frame_type is required for this frame class.")?;
    let frame_type = class
        .types
        .iter()
        .find(|item| item.id == requested_type)
        .ok_or_else(|| {
            format!(
                "INVALID_FRAME_COMBINATION: {requested_type} is not supported for {}.",
                selection.frame_class
            )
        })?;
    Ok((class.parameter_value, frame_type.parameter_value))
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
pub fn current_frame(service: State<'_, CalibrationService>) -> Result<FrameConfiguration, String> {
    let receiver =
        {
            let session = service.0.lock().unwrap();
            match session.status.firmware {
                Some(Firmware::ArduPilot) => {}
                Some(Firmware::Px4) => {
                    return Err(
                        "UNSUPPORTED_FEATURE: PX4 does not use FRAME_CLASS and FRAME_TYPE.".into(),
                    )
                }
                None => return Err(
                    "Connect an ArduPilot flight controller before reading frame configuration."
                        .into(),
                ),
            }
            let (reply, receiver) = mpsc::channel();
            session
                .sender
                .as_ref()
                .ok_or("Vehicle connection is unavailable.")?
                .send(WorkerCommand::ReadFrame(reply))
                .map_err(|_| "Vehicle connection was lost.".to_string())?;
            receiver
        };
    receiver.recv_timeout(Duration::from_secs(6)).map_err(|_| {
        "Timed out reading FRAME_CLASS and FRAME_TYPE from the flight controller.".to_string()
    })?
}

#[tauri::command]
pub fn set_frame(
    selection: FrameSelection,
    service: State<'_, CalibrationService>,
) -> Result<FrameUpdateResponse, String> {
    let (class_value, type_value) = resolve_frame_selection(&selection)?;
    let receiver =
        {
            let session = service.0.lock().unwrap();
            match session.status.firmware {
                Some(Firmware::ArduPilot) => {}
                Some(Firmware::Px4) => {
                    return Err(
                        "UNSUPPORTED_FEATURE: PX4 does not use FRAME_CLASS and FRAME_TYPE.".into(),
                    )
                }
                None => return Err(
                    "Connect an ArduPilot flight controller before changing frame configuration."
                        .into(),
                ),
            }
            match session.status.armed {
                Some(false) => {}
                Some(true) => return Err(
                    "VEHICLE_ARMED: Disarm the vehicle before changing its frame configuration."
                        .into(),
                ),
                None => {
                    return Err(
                        "Waiting for vehicle arm state. Try again after a heartbeat.".into(),
                    )
                }
            }
            let (reply, receiver) = mpsc::channel();
            session
                .sender
                .as_ref()
                .ok_or("Vehicle connection is unavailable.")?
                .send(WorkerCommand::SetFrame {
                    selection,
                    class_value,
                    type_value,
                    reply,
                })
                .map_err(|_| "Vehicle connection was lost.".to_string())?;
            receiver
        };
    receiver
        .recv_timeout(Duration::from_secs(12))
        .map_err(|_| "Timed out writing and verifying FRAME_CLASS and FRAME_TYPE.".to_string())?
}

fn parameter_id(name: &str) -> [u8; 16] {
    let mut result = [0; 16];
    let bytes = name.as_bytes();
    let length = bytes.len().min(result.len());
    result[..length].copy_from_slice(&bytes[..length]);
    result
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
    StartCompass {
        mask: u8,
        retry_on_failure: bool,
        autosave: bool,
    },
    CompassAction {
        command: mav::MavCmd,
        mask: u8,
    },
    Continue,
    Stop,
    ReadFrame(mpsc::Sender<Result<FrameConfiguration, String>>),
    SetFrame {
        selection: FrameSelection,
        class_value: u8,
        type_value: u8,
        reply: mpsc::Sender<Result<FrameUpdateResponse, String>>,
    },
    Disconnect,
}

struct PendingFrameWrite {
    selection: FrameSelection,
    class_value: f32,
    type_value: f32,
    class_seen: Option<f32>,
    type_seen: Option<f32>,
    verifying: bool,
    started: Instant,
    reply: mpsc::Sender<Result<FrameUpdateResponse, String>>,
}

#[derive(Default)]
struct Session {
    status: VehicleStatus,
    sender: Option<mpsc::Sender<WorkerCommand>>,
    generation: u64,
    accelerometer_session_id: Option<String>,
    accelerometer_requested_position: Option<String>,
    accelerometer_completed_steps: u8,
    compass_session_id: Option<String>,
    compass_mask: u8,
    compass_retry_on_failure: bool,
    compass_autosave: bool,
    compass_sensors: Vec<CompassSensorStatus>,
}

#[derive(Default)]
pub struct CalibrationService(Mutex<Session>);

#[derive(Clone, Debug, Serialize)]
pub struct CompassSensorStatus {
    pub compass_id: u8,
    pub progress_percent: u8,
    pub calibration_state: String,
    pub fitness: Option<f32>,
    pub fitness_source: &'static str,
    pub saved: bool,
    pub failure_reason: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CompassCalibrationStatus {
    pub session_id: String,
    pub status: CalibrationPhase,
    pub instruction: String,
    pub sensors: Vec<CompassSensorStatus>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CompassStartRequest {
    #[serde(default)]
    pub compass_ids: Vec<u8>,
    #[serde(default)]
    pub retry_on_failure: bool,
    #[serde(default)]
    pub autosave: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CompassSessionRequest {
    pub session_id: String,
}

fn compass_status(session: &Session) -> Result<CompassCalibrationStatus, String> {
    let session_id = session
        .compass_session_id
        .clone()
        .ok_or("No compass calibration session exists.")?;
    let progress = session
        .status
        .calibration
        .as_ref()
        .filter(|progress| progress.kind == CalibrationKind::Compass)
        .ok_or("No compass calibration session exists.")?;
    Ok(CompassCalibrationStatus {
        session_id,
        status: progress.phase.clone(),
        instruction: progress.message.clone(),
        sensors: session.compass_sensors.clone(),
    })
}

#[derive(Clone, Debug, Serialize)]
pub struct AccelerometerCalibrationStatus {
    pub session_id: String,
    pub status: CalibrationPhase,
    pub current_step: u8,
    pub total_steps: u8,
    pub orientation: Option<String>,
    pub instruction: String,
    pub waiting_for_confirmation: bool,
    pub percent: Option<u8>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AccelerometerStepRequest {
    pub session_id: String,
    pub step: String,
    pub action: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AccelerometerSessionRequest {
    pub session_id: String,
}

fn accelerometer_status(session: &Session) -> Result<AccelerometerCalibrationStatus, String> {
    let session_id = session
        .accelerometer_session_id
        .clone()
        .ok_or("No accelerometer calibration session exists.")?;
    let progress = session
        .status
        .calibration
        .as_ref()
        .filter(|progress| progress.kind == CalibrationKind::Accelerometer)
        .ok_or("No accelerometer calibration session exists.")?;
    Ok(AccelerometerCalibrationStatus {
        session_id,
        status: progress.phase.clone(),
        current_step: (session.accelerometer_completed_steps + 1).min(6),
        total_steps: 6,
        orientation: session.accelerometer_requested_position.clone(),
        instruction: progress.message.clone(),
        waiting_for_confirmation: matches!(progress.phase, CalibrationPhase::AwaitingUser),
        percent: progress.percent,
    })
}

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
    let (snapshot, accelerometer, compass) = {
        let mut session = service.0.lock().unwrap();
        if session.generation != generation {
            return;
        }
        f(&mut session);
        (
            session.status.clone(),
            accelerometer_status(&session).ok(),
            compass_status(&session).ok(),
        )
    };
    let _ = app.emit("datum://vehicle", snapshot);
    if let Some(accelerometer) = accelerometer {
        let _ = app.emit("datum://accelerometer", accelerometer);
    }
    if let Some(compass) = compass {
        let _ = app.emit("datum://compass", compass);
    }
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
    start_calibration_inner(kind, &service)
}

fn start_calibration_inner(
    kind: CalibrationKind,
    service: &CalibrationService,
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
pub fn start_accelerometer_calibration(
    calibration_type: Option<String>,
    service: State<'_, CalibrationService>,
) -> Result<AccelerometerCalibrationStatus, String> {
    if calibration_type
        .as_deref()
        .is_some_and(|kind| kind != "full")
    {
        return Err(
            "INVALID_CALIBRATION_TYPE: only full six-position calibration is supported.".into(),
        );
    }
    start_calibration_inner(CalibrationKind::Accelerometer, &service)?;
    let mut session = service.0.lock().unwrap();
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    session.accelerometer_session_id = Some(format!("accel-{}-{millis}", session.generation));
    session.accelerometer_requested_position = None;
    session.accelerometer_completed_steps = 0;
    accelerometer_status(&session)
}

#[tauri::command]
pub fn accelerometer_calibration_status(
    service: State<'_, CalibrationService>,
) -> Result<AccelerometerCalibrationStatus, String> {
    accelerometer_status(&service.0.lock().unwrap())
}

#[tauri::command]
pub fn accelerometer_calibration_step(
    request: AccelerometerStepRequest,
    service: State<'_, CalibrationService>,
) -> Result<AccelerometerCalibrationStatus, String> {
    let mut session = service.0.lock().unwrap();
    if session.accelerometer_session_id.as_deref() != Some(&request.session_id) {
        return Err("INVALID_SESSION: accelerometer calibration session does not match.".into());
    }
    if request.action != "confirm_position" {
        return Err("INVALID_ACTION: expected confirm_position.".into());
    }
    if session.accelerometer_requested_position.as_deref() != Some(&request.step) {
        return Err(format!(
            "POSITION_MISMATCH: the flight controller requested {}.",
            session
                .accelerometer_requested_position
                .as_deref()
                .unwrap_or("no position yet")
        ));
    }
    if !matches!(
        session.status.calibration.as_ref().map(|item| &item.phase),
        Some(CalibrationPhase::AwaitingUser)
    ) {
        return Err("The flight controller is not waiting for position confirmation.".into());
    }
    session
        .sender
        .as_ref()
        .ok_or("Vehicle connection is unavailable.")?
        .send(WorkerCommand::Continue)
        .map_err(|_| "Vehicle connection was lost.".to_string())?;
    if let Some(progress) = &mut session.status.calibration {
        progress.phase = CalibrationPhase::InProgress;
        progress.message = "Position confirmation sent; waiting for controller feedback".into();
    }
    accelerometer_status(&session)
}

#[tauri::command]
pub fn cancel_accelerometer_calibration(
    request: AccelerometerSessionRequest,
    service: State<'_, CalibrationService>,
) -> Result<AccelerometerCalibrationStatus, String> {
    let session = service.0.lock().unwrap();
    if session.accelerometer_session_id.as_deref() != Some(&request.session_id) {
        return Err("INVALID_SESSION: accelerometer calibration session does not match.".into());
    }
    session
        .sender
        .as_ref()
        .ok_or("Vehicle connection is unavailable.")?
        .send(WorkerCommand::Stop)
        .map_err(|_| "Vehicle connection was lost.".to_string())?;
    accelerometer_status(&session)
}

fn compass_mask(compass_ids: &[u8]) -> Result<u8, String> {
    if compass_ids.iter().any(|id| *id > 7) {
        return Err("INVALID_COMPASS_ID: compass IDs must be between 0 and 7.".into());
    }
    Ok(compass_ids.iter().fold(0u8, |mask, id| mask | (1u8 << id)))
}

#[tauri::command]
pub fn compass_sensors(
    service: State<'_, CalibrationService>,
) -> Result<Vec<CompassSensorStatus>, String> {
    let session = service.0.lock().unwrap();
    if !session.compass_sensors.is_empty() {
        return Ok(session.compass_sensors.clone());
    }
    let detected = session.status.sensors.iter().any(|sensor| {
        sensor.kind == CalibrationKind::Compass && sensor.condition != SensorCondition::Unknown
    });
    if !detected {
        return Ok(Vec::new());
    }
    Ok(vec![CompassSensorStatus {
        compass_id: 0,
        progress_percent: 0,
        calibration_state: "detected".into(),
        fitness: None,
        fitness_source: "not_available",
        saved: false,
        failure_reason: None,
    }])
}

#[tauri::command]
pub fn start_compass_calibration(
    request: CompassStartRequest,
    service: State<'_, CalibrationService>,
) -> Result<CompassCalibrationStatus, String> {
    let mask = compass_mask(&request.compass_ids)?;
    let mut session = service.0.lock().unwrap();
    if session.status.firmware != Some(Firmware::ArduPilot) {
        return Err(
            "UNSUPPORTED_FEATURE: onboard compass calibration currently requires ArduPilot.".into(),
        );
    }
    if session.status.armed != Some(false) {
        return Err("Disarm the vehicle before compass calibration.".into());
    }
    if session.status.calibration.as_ref().is_some_and(|progress| {
        matches!(
            progress.phase,
            CalibrationPhase::Starting
                | CalibrationPhase::Accepted
                | CalibrationPhase::InProgress
                | CalibrationPhase::AwaitingUser
                | CalibrationPhase::Stopping
        )
    }) {
        return Err("Finish or cancel the active calibration first.".into());
    }
    session
        .sender
        .as_ref()
        .ok_or("Vehicle connection is unavailable.")?
        .send(WorkerCommand::StartCompass {
            mask,
            retry_on_failure: request.retry_on_failure,
            autosave: request.autosave,
        })
        .map_err(|_| "Vehicle connection was lost.".to_string())?;
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    session.compass_session_id = Some(format!("compass-{}-{millis}", session.generation));
    session.compass_mask = mask;
    session.compass_retry_on_failure = request.retry_on_failure;
    session.compass_autosave = request.autosave;
    session.compass_sensors = request
        .compass_ids
        .iter()
        .map(|id| CompassSensorStatus {
            compass_id: *id,
            progress_percent: 0,
            calibration_state: "starting".into(),
            fitness: None,
            fitness_source: "not_available",
            saved: false,
            failure_reason: None,
        })
        .collect();
    session.status.calibration = Some(CalibrationProgress {
        kind: CalibrationKind::Compass,
        phase: CalibrationPhase::Starting,
        percent: None,
        message: "Starting onboard compass calibration".into(),
    });
    compass_status(&session)
}

#[tauri::command]
pub fn compass_calibration_status(
    service: State<'_, CalibrationService>,
) -> Result<CompassCalibrationStatus, String> {
    compass_status(&service.0.lock().unwrap())
}

fn send_compass_action(
    request: CompassSessionRequest,
    command: mav::MavCmd,
    service: &CalibrationService,
) -> Result<CompassCalibrationStatus, String> {
    let session = service.0.lock().unwrap();
    if session.compass_session_id.as_deref() != Some(request.session_id.as_str()) {
        return Err("INVALID_SESSION: compass calibration session does not match.".into());
    }
    session
        .sender
        .as_ref()
        .ok_or("Vehicle connection is unavailable.")?
        .send(WorkerCommand::CompassAction {
            command,
            mask: session.compass_mask,
        })
        .map_err(|_| "Vehicle connection was lost.".to_string())?;
    compass_status(&session)
}

#[tauri::command]
pub fn accept_compass_calibration(
    request: CompassSessionRequest,
    service: State<'_, CalibrationService>,
) -> Result<CompassCalibrationStatus, String> {
    send_compass_action(request, mav::MavCmd::MAV_CMD_DO_ACCEPT_MAG_CAL, &service)
}

#[tauri::command]
pub fn cancel_compass_calibration(
    request: CompassSessionRequest,
    service: State<'_, CalibrationService>,
) -> Result<CompassCalibrationStatus, String> {
    send_compass_action(request, mav::MavCmd::MAV_CMD_DO_CANCEL_MAG_CAL, &service)
}

#[tauri::command]
pub fn retry_compass_calibration(
    request: CompassSessionRequest,
    service: State<'_, CalibrationService>,
) -> Result<CompassCalibrationStatus, String> {
    let mut session = service.0.lock().unwrap();
    if session.compass_session_id.as_deref() != Some(request.session_id.as_str()) {
        return Err("INVALID_SESSION: compass calibration session does not match.".into());
    }
    session
        .sender
        .as_ref()
        .ok_or("Vehicle connection is unavailable.")?
        .send(WorkerCommand::StartCompass {
            mask: session.compass_mask,
            retry_on_failure: session.compass_retry_on_failure,
            autosave: session.compass_autosave,
        })
        .map_err(|_| "Vehicle connection was lost.".to_string())?;
    for sensor in &mut session.compass_sensors {
        sensor.progress_percent = 0;
        sensor.calibration_state = "starting".into();
        sensor.fitness = None;
        sensor.saved = false;
        sensor.failure_reason = None;
    }
    session.status.calibration = Some(CalibrationProgress {
        kind: CalibrationKind::Compass,
        phase: CalibrationPhase::Starting,
        percent: None,
        message: "Retrying onboard compass calibration".into(),
    });
    compass_status(&session)
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
    let mut compass_action: Option<mav::MavCmd> = None;
    let mut awaiting_position: Option<f32> = None;
    let mut pending_frame_read: Option<(
        mpsc::Sender<Result<FrameConfiguration, String>>,
        Option<f32>,
        Option<f32>,
        Instant,
    )> = None;
    let mut pending_frame_write: Option<PendingFrameWrite> = None;
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
                WorkerCommand::SetFrame {
                    selection,
                    class_value,
                    type_value,
                    reply,
                } => {
                    if pending_frame_read.is_some() || pending_frame_write.is_some() {
                        let _ = reply.send(Err(
                            "Another frame parameter operation is already in progress.".into(),
                        ));
                        continue;
                    }
                    let status = service.0.lock().unwrap().status.clone();
                    let (Some(system_id), Some(component_id)) =
                        (status.system_id, status.component_id)
                    else {
                        let _ = reply.send(Err("Vehicle identity is not available yet.".into()));
                        continue;
                    };
                    let mut send_error = None;
                    for (name, value) in [
                        ("FRAME_CLASS", class_value as f32),
                        ("FRAME_TYPE", type_value as f32),
                    ] {
                        sequence = sequence.wrapping_add(1);
                        let header = MavHeader {
                            system_id: 255,
                            component_id: 190,
                            sequence,
                        };
                        let parameter = mav::PARAM_SET_DATA {
                            target_system: system_id,
                            target_component: component_id,
                            param_id: parameter_id(name).into(),
                            param_value: value,
                            param_type: mav::MavParamType::MAV_PARAM_TYPE_REAL32,
                        };
                        if let Err(error) =
                            connection.send(&header, &mav::MavMessage::PARAM_SET(parameter))
                        {
                            send_error = Some(format!("Could not write {name}: {error}"));
                            break;
                        }
                    }
                    if let Some(error) = send_error {
                        let _ = reply.send(Err(error));
                    } else {
                        pending_frame_write = Some(PendingFrameWrite {
                            selection,
                            class_value: class_value as f32,
                            type_value: type_value as f32,
                            class_seen: None,
                            type_seen: None,
                            verifying: false,
                            started: Instant::now(),
                            reply,
                        });
                    }
                }
                WorkerCommand::ReadFrame(reply) => {
                    if pending_frame_read.is_some() || pending_frame_write.is_some() {
                        let _ = reply
                            .send(Err("A frame parameter read is already in progress.".into()));
                        continue;
                    }
                    let status = service.0.lock().unwrap().status.clone();
                    let (Some(system_id), Some(component_id)) =
                        (status.system_id, status.component_id)
                    else {
                        let _ = reply.send(Err("Vehicle identity is not available yet.".into()));
                        continue;
                    };
                    let mut send_error = None;
                    for name in ["FRAME_CLASS", "FRAME_TYPE"] {
                        sequence = sequence.wrapping_add(1);
                        let header = MavHeader {
                            system_id: 255,
                            component_id: 190,
                            sequence,
                        };
                        let request = mav::PARAM_REQUEST_READ_DATA {
                            target_system: system_id,
                            target_component: component_id,
                            param_id: parameter_id(name).into(),
                            param_index: -1,
                        };
                        if let Err(error) =
                            connection.send(&header, &mav::MavMessage::PARAM_REQUEST_READ(request))
                        {
                            send_error = Some(format!("Could not request {name}: {error}"));
                            break;
                        }
                    }
                    if let Some(error) = send_error {
                        let _ = reply.send(Err(error));
                    } else {
                        pending_frame_read = Some((reply, None, None, Instant::now()));
                    }
                }
                WorkerCommand::StartCompass {
                    mask,
                    retry_on_failure,
                    autosave,
                } => {
                    let status = service.0.lock().unwrap().status.clone();
                    let (Some(system_id), Some(component_id)) =
                        (status.system_id, status.component_id)
                    else {
                        continue;
                    };
                    for message_id in [191.0, 192.0] {
                        sequence = sequence.wrapping_add(1);
                        let header = MavHeader {
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
                        let _ = connection.send(&header, &mav::MavMessage::COMMAND_LONG(request));
                    }
                    sequence = sequence.wrapping_add(1);
                    let header = MavHeader {
                        system_id: 255,
                        component_id: 190,
                        sequence,
                    };
                    let request = mav::COMMAND_LONG_DATA {
                        target_system: system_id,
                        target_component: component_id,
                        command: mav::MavCmd::MAV_CMD_DO_START_MAG_CAL,
                        confirmation: 0,
                        param1: mask as f32,
                        param2: u8::from(retry_on_failure) as f32,
                        param3: u8::from(autosave) as f32,
                        param4: 0.0,
                        param5: 0.0,
                        param6: 0.0,
                        param7: 0.0,
                    };
                    match connection.send(&header, &mav::MavMessage::COMMAND_LONG(request)) {
                        Ok(_) => {
                            pending = Some((
                                CalibrationKind::Compass,
                                mav::MavCmd::MAV_CMD_DO_START_MAG_CAL,
                                Instant::now(),
                            ));
                            compass_expected = mask;
                            compass_success = 0;
                        }
                        Err(error) => change(&app, &service, generation, |s| {
                            if let Some(progress) = &mut s.status.calibration {
                                progress.phase = CalibrationPhase::Failed;
                                progress.message =
                                    format!("Could not start compass calibration: {error}");
                            }
                        }),
                    }
                }
                WorkerCommand::CompassAction { command, mask } => {
                    let status = service.0.lock().unwrap().status.clone();
                    let (Some(system_id), Some(component_id)) =
                        (status.system_id, status.component_id)
                    else {
                        continue;
                    };
                    sequence = sequence.wrapping_add(1);
                    let header = MavHeader {
                        system_id: 255,
                        component_id: 190,
                        sequence,
                    };
                    let request = mav::COMMAND_LONG_DATA {
                        target_system: system_id,
                        target_component: component_id,
                        command,
                        confirmation: 0,
                        param1: mask as f32,
                        param2: 0.0,
                        param3: 0.0,
                        param4: 0.0,
                        param5: 0.0,
                        param6: 0.0,
                        param7: 0.0,
                    };
                    if connection
                        .send(&header, &mav::MavMessage::COMMAND_LONG(request))
                        .is_ok()
                    {
                        compass_action = Some(command);
                    }
                }
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
                                    s.accelerometer_completed_steps =
                                        (s.accelerometer_completed_steps + 1).min(6);
                                    s.accelerometer_requested_position = None;
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
                    if compass_action == Some(ack.command) {
                        let action = compass_action.take();
                        let accepted = ack.result == mav::MavResult::MAV_RESULT_ACCEPTED;
                        change(&app, &service, generation, |s| {
                            if let Some(progress) = &mut s.status.calibration {
                                if accepted
                                    && action == Some(mav::MavCmd::MAV_CMD_DO_CANCEL_MAG_CAL)
                                {
                                    progress.phase = CalibrationPhase::Stopped;
                                    progress.message =
                                        "Compass calibration cancelled by the flight controller"
                                            .into();
                                } else if accepted {
                                    progress.message =
                                        "Compass calibration results accepted by the flight controller".into();
                                } else {
                                    progress.phase = CalibrationPhase::Failed;
                                    progress.message = format!(
                                        "Flight controller rejected compass action: {:?}",
                                        ack.result
                                    );
                                }
                            }
                        });
                        if action == Some(mav::MavCmd::MAV_CMD_DO_CANCEL_MAG_CAL) {
                            pending = None;
                        }
                        continue;
                    }
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
                mav::MavMessage::PARAM_VALUE(parameter) => {
                    let parameter_name = parameter.param_id.to_str().unwrap_or_default();
                    if let Some(mut write) = pending_frame_write.take() {
                        match parameter_name {
                            "FRAME_CLASS" => write.class_seen = Some(parameter.param_value),
                            "FRAME_TYPE" => write.type_seen = Some(parameter.param_value),
                            _ => {}
                        }
                        if let (Some(class_value), Some(type_value)) =
                            (write.class_seen, write.type_seen)
                        {
                            let values_match = (class_value - write.class_value).abs() < 0.01
                                && (type_value - write.type_value).abs() < 0.01;
                            if !values_match {
                                let _ = write.reply.send(Err(format!(
                                    "PARAMETER_VERIFICATION_FAILED: expected FRAME_CLASS={} and FRAME_TYPE={}, received {class_value} and {type_value}.",
                                    write.class_value, write.type_value
                                )));
                            } else if write.verifying {
                                let _ = write.reply.send(Ok(FrameUpdateResponse {
                                    success: true,
                                    verified: true,
                                    frame_class: write.selection.frame_class,
                                    frame_type: write.selection.frame_type,
                                    reboot_required: true,
                                }));
                            } else {
                                let status = service.0.lock().unwrap().status.clone();
                                if let (Some(system_id), Some(component_id)) =
                                    (status.system_id, status.component_id)
                                {
                                    let mut request_error = None;
                                    for name in ["FRAME_CLASS", "FRAME_TYPE"] {
                                        sequence = sequence.wrapping_add(1);
                                        let request_header = MavHeader {
                                            system_id: 255,
                                            component_id: 190,
                                            sequence,
                                        };
                                        let request = mav::PARAM_REQUEST_READ_DATA {
                                            target_system: system_id,
                                            target_component: component_id,
                                            param_id: parameter_id(name).into(),
                                            param_index: -1,
                                        };
                                        if let Err(error) = connection.send(
                                            &request_header,
                                            &mav::MavMessage::PARAM_REQUEST_READ(request),
                                        ) {
                                            request_error = Some(format!(
                                                "Could not verify {name} after writing: {error}"
                                            ));
                                            break;
                                        }
                                    }
                                    if let Some(error) = request_error {
                                        let _ = write.reply.send(Err(error));
                                    } else {
                                        write.class_seen = None;
                                        write.type_seen = None;
                                        write.verifying = true;
                                        write.started = Instant::now();
                                        pending_frame_write = Some(write);
                                    }
                                } else {
                                    let _ = write.reply.send(Err(
                                        "Vehicle identity was lost before parameter verification."
                                            .into(),
                                    ));
                                }
                            }
                        } else {
                            pending_frame_write = Some(write);
                        }
                    }
                    if let Some((_, class_value, type_value, _)) = &mut pending_frame_read {
                        match parameter_name {
                            "FRAME_CLASS" => *class_value = Some(parameter.param_value),
                            "FRAME_TYPE" => *type_value = Some(parameter.param_value),
                            _ => {}
                        }
                    }
                    let completed =
                        pending_frame_read
                            .as_ref()
                            .and_then(|(_, class_value, type_value, _)| {
                                match (*class_value, *type_value) {
                                    (Some(class_value), Some(type_value)) => {
                                        Some(resolve_frame_configuration(class_value, type_value))
                                    }
                                    _ => None,
                                }
                            });
                    if let Some(configuration) = completed {
                        if let Some((reply, _, _, _)) = pending_frame_read.take() {
                            let _ = reply.send(Ok(configuration));
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
                                s.accelerometer_requested_position =
                                    position_name(request.param1).map(str::to_string);
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
                                s.accelerometer_completed_steps = 6;
                                s.accelerometer_requested_position = None;
                                s.status.calibration = Some(CalibrationProgress {
                                kind: CalibrationKind::Accelerometer, phase: CalibrationPhase::Completed,
                                percent: Some(100), message: "Accelerometer calibration completed according to the flight controller".into(),
                            })
                            });
                        } else if request.param1 as u32 == 16777216 {
                            pending = None;
                            change(&app, &service, generation, |s| {
                                s.accelerometer_requested_position = None;
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
                            let sensor = if let Some(sensor) = s
                                .compass_sensors
                                .iter_mut()
                                .find(|sensor| sensor.compass_id == progress.compass_id)
                            {
                                sensor
                            } else {
                                s.compass_sensors.push(CompassSensorStatus {
                                    compass_id: progress.compass_id,
                                    progress_percent: 0,
                                    calibration_state: "running".into(),
                                    fitness: None,
                                    fitness_source: "not_available",
                                    saved: false,
                                    failure_reason: None,
                                });
                                s.compass_sensors.last_mut().unwrap()
                            };
                            sensor.progress_percent = progress.completion_pct;
                            sensor.calibration_state = "running".into();
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
                            let succeeded = report.cal_status == mav::MagCalStatus::MAG_CAL_SUCCESS;
                            let autosave_required = service.0.lock().unwrap().compass_autosave;
                            if succeeded && (!autosave_required || report.autosaved == 1) {
                                compass_success |= bit;
                                change(&app, &service, generation, |s| {
                                    let sensor = if let Some(sensor) = s
                                        .compass_sensors
                                        .iter_mut()
                                        .find(|sensor| sensor.compass_id == report.compass_id)
                                    {
                                        sensor
                                    } else {
                                        s.compass_sensors.push(CompassSensorStatus {
                                            compass_id: report.compass_id,
                                            progress_percent: 100,
                                            calibration_state: "report_received".into(),
                                            fitness: None,
                                            fitness_source: "firmware_report",
                                            saved: false,
                                            failure_reason: None,
                                        });
                                        s.compass_sensors.last_mut().unwrap()
                                    };
                                    sensor.progress_percent = 100;
                                    sensor.calibration_state = if report.autosaved == 1 {
                                        "saved".into()
                                    } else {
                                        "fit_complete_awaiting_accept".into()
                                    };
                                    sensor.fitness = Some(report.fitness);
                                    sensor.fitness_source = "firmware_report";
                                    sensor.saved = report.autosaved == 1;
                                    sensor.failure_reason = None;
                                });
                            } else {
                                pending = None;
                                stopping = None;
                                change(&app, &service, generation, |s| {
                                    if let Some(sensor) = s
                                        .compass_sensors
                                        .iter_mut()
                                        .find(|sensor| sensor.compass_id == report.compass_id)
                                    {
                                        sensor.calibration_state = "failed".into();
                                        sensor.fitness = Some(report.fitness);
                                        sensor.fitness_source = "firmware_report";
                                        sensor.failure_reason =
                                            Some(format!("{:?}", report.cal_status));
                                    }
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
                                    kind: CalibrationKind::Compass,
                                    phase: CalibrationPhase::Completed,
                                    percent: Some(100),
                                    message: if s.compass_autosave {
                                        "All reported compasses calibrated and saved. Reboot the vehicle before flight.".into()
                                    } else {
                                        "All reported compass fits succeeded. Accept the results to save them.".into()
                                    },
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
        if pending_frame_read
            .as_ref()
            .is_some_and(|(_, _, _, started)| started.elapsed() > Duration::from_secs(5))
        {
            if let Some((reply, _, _, _)) = pending_frame_read.take() {
                let _ = reply.send(Err(
                    "Timed out waiting for FRAME_CLASS or FRAME_TYPE from the flight controller."
                        .into(),
                ));
            }
        }
        if pending_frame_write
            .as_ref()
            .is_some_and(|write| write.started.elapsed() > Duration::from_secs(5))
        {
            if let Some(write) = pending_frame_write.take() {
                let phase = if write.verifying {
                    "readback verification"
                } else {
                    "parameter write acknowledgement"
                };
                let _ = write.reply.send(Err(format!(
                    "Timed out waiting for frame {phase} from the flight controller."
                )));
            }
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
    fn frame_parameter_values_resolve_without_hiding_unknown_values() {
        let known = resolve_frame_configuration(1.0, 1.0);
        assert_eq!(known.frame_class.as_deref(), Some("quad"));
        assert_eq!(known.frame_type.as_deref(), Some("x"));
        assert!(!known.reboot_required);

        let unknown = resolve_frame_configuration(99.0, 42.0);
        assert_eq!(unknown.frame_class, None);
        assert_eq!(unknown.frame_type, None);
        assert_eq!(unknown.frame_class_value, 99.0);
        assert_eq!(unknown.frame_type_value, 42.0);
    }

    #[test]
    fn frame_selection_rejects_invalid_combinations() {
        assert_eq!(
            resolve_frame_selection(&FrameSelection {
                frame_class: "quad".into(),
                frame_type: Some("x".into()),
            })
            .unwrap(),
            (1, 1)
        );
        assert!(resolve_frame_selection(&FrameSelection {
            frame_class: "y6".into(),
            frame_type: Some("x".into()),
        })
        .unwrap_err()
        .starts_with("INVALID_FRAME_COMBINATION"));
        assert!(resolve_frame_selection(&FrameSelection {
            frame_class: "tri".into(),
            frame_type: Some("x".into()),
        })
        .is_err());
        assert_eq!(
            resolve_frame_selection(&FrameSelection {
                frame_class: "tri".into(),
                frame_type: None,
            })
            .unwrap(),
            (7, 0)
        );
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
        assert_eq!(compass_mask(&[]).unwrap(), 0);
        assert_eq!(compass_mask(&[0, 2]).unwrap(), 5);
        assert!(compass_mask(&[8]).is_err());
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
        assert_eq!(position_name(1.0), Some("level"));
        assert_eq!(position_name(6.0), Some("upside_down"));
        assert_eq!(position_name(9.0), None);
    }
}
