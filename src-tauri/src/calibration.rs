use mavlink::{ardupilotmega as mav, MavConnection, MavHeader};
use serde::{Deserialize, Serialize};
use serialport::{SerialPortInfo, SerialPortType};
use std::{
    collections::BTreeMap,
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

fn read_parameters(
    service: &CalibrationService,
    names: &[&str],
) -> Result<BTreeMap<String, f32>, String> {
    let receiver = {
        let session = service.0.lock().unwrap();
        let (reply, receiver) = mpsc::channel();
        session
            .sender
            .as_ref()
            .ok_or("Vehicle connection is unavailable.")?
            .send(WorkerCommand::ReadParameters {
                names: names.iter().map(|name| (*name).to_string()).collect(),
                reply,
            })
            .map_err(|_| "Vehicle connection was lost.".to_string())?;
        receiver
    };
    receiver
        .recv_timeout(Duration::from_secs(8))
        .map_err(|_| "Timed out waiting for flight-controller parameters.".to_string())?
}

fn write_parameters(
    service: &CalibrationService,
    parameters: BTreeMap<String, f32>,
) -> Result<(), String> {
    let receiver = {
        let session = service.0.lock().unwrap();
        if session.status.armed != Some(false) {
            return Err("Disarm the vehicle before changing configuration parameters.".into());
        }
        let (reply, receiver) = mpsc::channel();
        session
            .sender
            .as_ref()
            .ok_or("Vehicle connection is unavailable.")?
            .send(WorkerCommand::WriteParameters { parameters, reply })
            .map_err(|_| "Vehicle connection was lost.".to_string())?;
        receiver
    };
    receiver
        .recv_timeout(Duration::from_secs(15))
        .map_err(|_| "Timed out writing flight-controller parameters.".to_string())?
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
    RequestRadioStream,
    ReadParameters {
        names: Vec<String>,
        reply: mpsc::Sender<Result<BTreeMap<String, f32>, String>>,
    },
    WriteParameters {
        parameters: BTreeMap<String, f32>,
        reply: mpsc::Sender<Result<(), String>>,
    },
    MotorTest {
        motor_number: u8,
        throttle_percent: f32,
        duration_seconds: f32,
        reply: mpsc::Sender<Result<MotorTestResponse, String>>,
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
    attempts: u8,
    reply: mpsc::Sender<Result<FrameUpdateResponse, String>>,
}

struct PendingParameterWrite {
    expected: BTreeMap<String, f32>,
    seen: BTreeMap<String, f32>,
    verifying: bool,
    started: Instant,
    attempts: u8,
    reply: mpsc::Sender<Result<(), String>>,
}

struct PendingParameterRead {
    names: Vec<String>,
    values: BTreeMap<String, f32>,
    started: Instant,
    attempts: u8,
    reply: mpsc::Sender<Result<BTreeMap<String, f32>, String>>,
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
    radio_session_id: Option<String>,
    radio_active: bool,
    radio_capture_min_max: bool,
    radio_capture_trim: bool,
    radio_channels: Vec<RadioChannelStatus>,
    esc_session_id: Option<String>,
    esc_step: usize,
    esc_active: bool,
    battery_voltage_v: Option<f32>,
    battery_current_a: Option<f32>,
    battery_remaining_percent: Option<i8>,
    active_mode: Option<u32>,
}

#[derive(Default)]
pub struct CalibrationService(Mutex<Session>);

#[derive(Clone, Debug, Serialize)]
pub struct RadioChannelStatus {
    pub channel: u8,
    pub value_us: u16,
    pub minimum_us: u16,
    pub maximum_us: u16,
    pub trim_us: u16,
    pub range_valid: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct RadioCalibrationStatus {
    pub session_id: Option<String>,
    pub status: String,
    pub signal_available: bool,
    pub channels: Vec<RadioChannelStatus>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RadioStartRequest {
    #[serde(default = "default_true")]
    pub capture_min_max: bool,
    #[serde(default = "default_true")]
    pub capture_trim: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RadioSessionRequest {
    pub session_id: String,
}

fn default_true() -> bool {
    true
}

fn radio_status(session: &Session) -> RadioCalibrationStatus {
    RadioCalibrationStatus {
        session_id: session.radio_session_id.clone(),
        status: if session.radio_active {
            "capturing".into()
        } else if session.radio_session_id.is_some() {
            "stopped".into()
        } else {
            "idle".into()
        },
        signal_available: !session.radio_channels.is_empty(),
        channels: session.radio_channels.clone(),
    }
}

fn radio_range_valid(minimum_us: u16, maximum_us: u16) -> bool {
    maximum_us.saturating_sub(minimum_us) >= 400
}

#[derive(Clone, Debug, Serialize)]
pub struct EscCapabilities {
    pub pwm_type_value: u8,
    pub protocol: String,
    pub endpoint_calibration_required: bool,
    pub software_can_verify_completion: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct EscWorkflowStatus {
    pub session_id: String,
    pub active: bool,
    pub step_id: String,
    pub instruction: String,
    pub manually_confirmed: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct EscStartRequest {
    pub propellers_removed: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct EscStepRequest {
    pub session_id: String,
    pub step_id: String,
    pub action: String,
}

const ESC_STEPS: [(&str, &str); 5] = [
    ("remove_propellers", "Remove every propeller and secure the vehicle."),
    ("disconnect_battery", "Disconnect flight battery power; keep USB connected only if your controller procedure permits it."),
    ("raise_throttle", "Move transmitter throttle to maximum."),
    ("power_and_confirm_tones", "Follow the ESC manufacturer's power-up procedure and confirm the calibration tones."),
    ("lower_throttle", "Move throttle to minimum and confirm the completion tones, then disconnect power."),
];

fn esc_protocol(value: u8) -> (&'static str, bool) {
    match value {
        4 => ("dshot150", false),
        5 => ("dshot300", false),
        6 => ("dshot600", false),
        7 => ("dshot1200", false),
        1 => ("oneshot", true),
        2 => ("oneshot125", true),
        3 => ("brushed", false),
        _ => ("normal_pwm", true),
    }
}

fn next_esc_step(current: usize, active: bool, requested_step: &str) -> Result<usize, String> {
    if !active || current >= ESC_STEPS.len() {
        return Err("ESC_WORKFLOW_INACTIVE: start a new ESC calibration workflow.".into());
    }
    if ESC_STEPS[current].0 != requested_step {
        return Err("INVALID_STEP: confirm the currently requested ESC workflow step.".into());
    }
    Ok(current + 1)
}

#[derive(Clone, Debug, Serialize)]
pub struct FlightModeConfiguration {
    pub mode_channel: u8,
    pub slots: Vec<u8>,
    pub active_mode: Option<u32>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct FlightModeUpdate {
    pub mode_channel: u8,
    pub slots: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FailsafeConfiguration {
    pub throttle_action: u8,
    pub battery_action: u8,
    pub battery_critical_action: u8,
    pub gcs_action: u8,
}

#[derive(Clone, Debug, Serialize)]
pub struct BatteryConfiguration {
    pub monitor_type: u8,
    pub capacity_mah: f32,
    pub voltage_multiplier: f32,
    pub amps_per_volt: f32,
    pub low_voltage: f32,
    pub critical_voltage: f32,
    pub voltage_v: Option<f32>,
    pub current_a: Option<f32>,
    pub remaining_percent: Option<i8>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct BatteryUpdate {
    pub monitor_type: u8,
    pub capacity_mah: f32,
    pub voltage_multiplier: f32,
    pub amps_per_volt: f32,
    pub low_voltage: f32,
    pub critical_voltage: f32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct BatteryVoltageCalibration {
    pub measured_voltage: f32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MotorTestRequest {
    pub motor_number: u8,
    pub throttle_percent: f32,
    pub duration_seconds: f32,
    pub propellers_removed: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct MotorTestResponse {
    pub accepted: bool,
    pub motor_number: u8,
    pub throttle_percent: f32,
    pub duration_seconds: f32,
    pub message: String,
}

fn motor_test_command(
    target_system: u8,
    target_component: u8,
    motor_number: u8,
    throttle_percent: f32,
    duration_seconds: f32,
) -> mav::COMMAND_LONG_DATA {
    mav::COMMAND_LONG_DATA {
        target_system,
        target_component,
        command: mav::MavCmd::MAV_CMD_DO_MOTOR_TEST,
        confirmation: 0,
        param1: motor_number as f32,
        param2: 0.0,
        param3: throttle_percent,
        param4: duration_seconds,
        param5: 0.0,
        param6: 0.0,
        param7: 0.0,
    }
}

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
    let (snapshot, accelerometer, compass, radio) = {
        let mut session = service.0.lock().unwrap();
        if session.generation != generation {
            return;
        }
        f(&mut session);
        (
            session.status.clone(),
            accelerometer_status(&session).ok(),
            compass_status(&session).ok(),
            radio_status(&session),
        )
    };
    let _ = app.emit("datum://vehicle", snapshot);
    if let Some(accelerometer) = accelerometer {
        let _ = app.emit("datum://accelerometer", accelerometer);
    }
    if let Some(compass) = compass {
        let _ = app.emit("datum://compass", compass);
    }
    let _ = app.emit("datum://radio", radio);
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
pub fn radio_channels(service: State<'_, CalibrationService>) -> RadioCalibrationStatus {
    radio_status(&service.0.lock().unwrap())
}

#[tauri::command]
pub fn start_radio_calibration(
    request: RadioStartRequest,
    service: State<'_, CalibrationService>,
) -> Result<RadioCalibrationStatus, String> {
    if !request.capture_min_max && !request.capture_trim {
        return Err("INVALID_REQUEST: enable range capture, trim capture, or both.".into());
    }
    let mut session = service.0.lock().unwrap();
    if session.status.firmware != Some(Firmware::ArduPilot) {
        return Err("UNSUPPORTED_FEATURE: this radio endpoint currently supports ArduPilot parameter calibration.".into());
    }
    if session.status.armed != Some(false) {
        return Err("Disarm the vehicle before radio calibration.".into());
    }
    session
        .sender
        .as_ref()
        .ok_or("Vehicle connection is unavailable.")?
        .send(WorkerCommand::RequestRadioStream)
        .map_err(|_| "Vehicle connection was lost.".to_string())?;
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    session.radio_session_id = Some(format!("radio-{}-{millis}", session.generation));
    session.radio_active = true;
    session.radio_capture_min_max = request.capture_min_max;
    session.radio_capture_trim = request.capture_trim;
    for channel in &mut session.radio_channels {
        channel.minimum_us = channel.value_us;
        channel.maximum_us = channel.value_us;
        channel.trim_us = channel.value_us;
        channel.range_valid = false;
    }
    Ok(radio_status(&session))
}

#[tauri::command]
pub fn radio_calibration_status(service: State<'_, CalibrationService>) -> RadioCalibrationStatus {
    radio_status(&service.0.lock().unwrap())
}

#[tauri::command]
pub fn finish_radio_calibration(
    request: RadioSessionRequest,
    service: State<'_, CalibrationService>,
) -> Result<RadioCalibrationStatus, String> {
    let (receiver, capture_trim) = {
        let session = service.0.lock().unwrap();
        if session.radio_session_id.as_deref() != Some(request.session_id.as_str()) {
            return Err("INVALID_SESSION: radio calibration session does not match.".into());
        }
        if !session.radio_active {
            return Err("Radio calibration is not capturing.".into());
        }
        if session.radio_channels.is_empty() {
            return Err("NO_RC_SIGNAL: no RC_CHANNELS telemetry has been received.".into());
        }
        if session.radio_capture_min_max {
            let invalid = session
                .radio_channels
                .iter()
                .filter(|channel| !radio_range_valid(channel.minimum_us, channel.maximum_us))
                .map(|channel| channel.channel.to_string())
                .collect::<Vec<_>>();
            if !invalid.is_empty() {
                return Err(format!(
                    "INVALID_RC_RANGE: move channels {} through their complete range before finishing.",
                    invalid.join(", ")
                ));
            }
        }
        let mut parameters = BTreeMap::new();
        for channel in &session.radio_channels {
            if session.radio_capture_min_max {
                parameters.insert(
                    format!("RC{}_MIN", channel.channel),
                    channel.minimum_us as f32,
                );
                parameters.insert(
                    format!("RC{}_MAX", channel.channel),
                    channel.maximum_us as f32,
                );
            }
            if session.radio_capture_trim {
                parameters.insert(
                    format!("RC{}_TRIM", channel.channel),
                    channel.trim_us as f32,
                );
            }
        }
        let (reply, receiver) = mpsc::channel();
        session
            .sender
            .as_ref()
            .ok_or("Vehicle connection is unavailable.")?
            .send(WorkerCommand::WriteParameters { parameters, reply })
            .map_err(|_| "Vehicle connection was lost.".to_string())?;
        (receiver, session.radio_capture_trim)
    };
    receiver
        .recv_timeout(Duration::from_secs(15))
        .map_err(|_| "Timed out saving and verifying radio parameters.".to_string())??;
    let mut session = service.0.lock().unwrap();
    session.radio_active = false;
    let mut status = radio_status(&session);
    status.status = if capture_trim {
        "saved_and_verified".into()
    } else {
        "ranges_saved_and_verified".into()
    };
    Ok(status)
}

#[tauri::command]
pub fn cancel_radio_calibration(
    request: RadioSessionRequest,
    service: State<'_, CalibrationService>,
) -> Result<RadioCalibrationStatus, String> {
    let mut session = service.0.lock().unwrap();
    if session.radio_session_id.as_deref() != Some(request.session_id.as_str()) {
        return Err("INVALID_SESSION: radio calibration session does not match.".into());
    }
    session.radio_active = false;
    Ok(radio_status(&session))
}

#[tauri::command]
pub fn esc_capabilities(service: State<'_, CalibrationService>) -> Result<EscCapabilities, String> {
    let values = read_parameters(&service, &["MOT_PWM_TYPE"])?;
    let value = values["MOT_PWM_TYPE"].round() as u8;
    let (protocol, required) = esc_protocol(value);
    Ok(EscCapabilities {
        pwm_type_value: value,
        protocol: protocol.into(),
        endpoint_calibration_required: required,
        software_can_verify_completion: false,
    })
}

#[tauri::command]
pub fn esc_steps() -> Vec<BTreeMap<String, String>> {
    ESC_STEPS
        .iter()
        .map(|(id, instruction)| {
            BTreeMap::from([
                ("step_id".into(), (*id).into()),
                ("instruction".into(), (*instruction).into()),
            ])
        })
        .collect()
}

fn esc_status(session: &Session) -> Result<EscWorkflowStatus, String> {
    let session_id = session
        .esc_session_id
        .clone()
        .ok_or("No ESC calibration workflow exists.")?;
    let index = session.esc_step.min(ESC_STEPS.len() - 1);
    Ok(EscWorkflowStatus {
        session_id,
        active: session.esc_active,
        step_id: ESC_STEPS[index].0.into(),
        instruction: ESC_STEPS[index].1.into(),
        manually_confirmed: !session.esc_active && session.esc_step >= ESC_STEPS.len(),
    })
}

#[tauri::command]
pub fn start_esc_calibration(
    request: EscStartRequest,
    service: State<'_, CalibrationService>,
) -> Result<EscWorkflowStatus, String> {
    if !request.propellers_removed {
        return Err("PROPELLERS_PRESENT: remove all propellers before ESC calibration.".into());
    }
    let capability = esc_capabilities(service.clone())?;
    if !capability.endpoint_calibration_required {
        return Err(format!(
            "UNSUPPORTED_FEATURE: {} does not require traditional PWM endpoint calibration.",
            capability.protocol
        ));
    }
    let mut session = service.0.lock().unwrap();
    if session.status.armed != Some(false) {
        return Err("Disarm the vehicle before ESC calibration.".into());
    }
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    session.esc_session_id = Some(format!("esc-{}-{millis}", session.generation));
    session.esc_step = 0;
    session.esc_active = true;
    esc_status(&session)
}

#[tauri::command]
pub fn advance_esc_calibration(
    request: EscStepRequest,
    service: State<'_, CalibrationService>,
) -> Result<EscWorkflowStatus, String> {
    let mut session = service.0.lock().unwrap();
    if session.esc_session_id.as_deref() != Some(request.session_id.as_str()) {
        return Err("INVALID_SESSION: ESC workflow session does not match.".into());
    }
    if request.action != "confirm_completed" {
        return Err("INVALID_STEP: confirm the currently requested ESC workflow step.".into());
    }
    session.esc_step = next_esc_step(session.esc_step, session.esc_active, &request.step_id)?;
    if session.esc_step >= ESC_STEPS.len() {
        session.esc_active = false;
    }
    esc_status(&session)
}

#[tauri::command]
pub fn esc_calibration_status(
    service: State<'_, CalibrationService>,
) -> Result<EscWorkflowStatus, String> {
    esc_status(&service.0.lock().unwrap())
}

#[tauri::command]
pub fn cancel_esc_calibration(
    request: RadioSessionRequest,
    service: State<'_, CalibrationService>,
) -> Result<EscWorkflowStatus, String> {
    let mut session = service.0.lock().unwrap();
    if session.esc_session_id.as_deref() != Some(request.session_id.as_str()) {
        return Err("INVALID_SESSION: ESC workflow session does not match.".into());
    }
    session.esc_active = false;
    esc_status(&session)
}

#[tauri::command]
pub fn flight_mode_configuration(
    service: State<'_, CalibrationService>,
) -> Result<FlightModeConfiguration, String> {
    let values = read_parameters(
        &service,
        &[
            "FLTMODE1",
            "FLTMODE2",
            "FLTMODE3",
            "FLTMODE4",
            "FLTMODE5",
            "FLTMODE6",
            "FLTMODE_CH",
        ],
    )?;
    Ok(FlightModeConfiguration {
        mode_channel: values["FLTMODE_CH"].round() as u8,
        slots: (1..=6)
            .map(|index| values[&format!("FLTMODE{index}")].round() as u8)
            .collect(),
        active_mode: service.0.lock().unwrap().active_mode,
    })
}

#[tauri::command]
pub fn set_flight_mode_configuration(
    request: FlightModeUpdate,
    service: State<'_, CalibrationService>,
) -> Result<FlightModeConfiguration, String> {
    if request.slots.len() != 6 || !(1..=16).contains(&request.mode_channel) {
        return Err(
            "INVALID_FLIGHT_MODE_CONFIGURATION: provide six slots and a channel from 1 to 16."
                .into(),
        );
    }
    const MODES: &[u8] = &[
        0, 1, 2, 3, 4, 5, 6, 7, 9, 11, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27,
    ];
    if request.slots.iter().any(|mode| !MODES.contains(mode)) {
        return Err(
            "UNSUPPORTED_FLIGHT_MODE: a slot contains an unknown ArduCopter mode value.".into(),
        );
    }
    let mut parameters = BTreeMap::from([("FLTMODE_CH".into(), request.mode_channel as f32)]);
    for (index, mode) in request.slots.iter().enumerate() {
        parameters.insert(format!("FLTMODE{}", index + 1), *mode as f32);
    }
    write_parameters(&service, parameters)?;
    flight_mode_configuration(service)
}

#[tauri::command]
pub fn failsafe_configuration(
    service: State<'_, CalibrationService>,
) -> Result<FailsafeConfiguration, String> {
    let values = read_parameters(
        &service,
        &[
            "FS_THR_ENABLE",
            "BATT_FS_LOW_ACT",
            "BATT_FS_CRT_ACT",
            "FS_GCS_ENABLE",
        ],
    )?;
    Ok(FailsafeConfiguration {
        throttle_action: values["FS_THR_ENABLE"].round() as u8,
        battery_action: values["BATT_FS_LOW_ACT"].round() as u8,
        battery_critical_action: values["BATT_FS_CRT_ACT"].round() as u8,
        gcs_action: values["FS_GCS_ENABLE"].round() as u8,
    })
}

#[tauri::command]
pub fn set_failsafe_configuration(
    request: FailsafeConfiguration,
    service: State<'_, CalibrationService>,
) -> Result<FailsafeConfiguration, String> {
    if request.throttle_action > 7
        || request.battery_action > 5
        || request.battery_critical_action > 5
        || request.gcs_action > 7
    {
        return Err("INVALID_FAILSAFE_ACTION: an action is outside the supported ArduPilot parameter range.".into());
    }
    write_parameters(
        &service,
        BTreeMap::from([
            ("FS_THR_ENABLE".into(), request.throttle_action as f32),
            ("BATT_FS_LOW_ACT".into(), request.battery_action as f32),
            (
                "BATT_FS_CRT_ACT".into(),
                request.battery_critical_action as f32,
            ),
            ("FS_GCS_ENABLE".into(), request.gcs_action as f32),
        ]),
    )?;
    failsafe_configuration(service)
}

fn battery_from_values(session: &Session, values: &BTreeMap<String, f32>) -> BatteryConfiguration {
    BatteryConfiguration {
        monitor_type: values["BATT_MONITOR"].round() as u8,
        capacity_mah: values["BATT_CAPACITY"],
        voltage_multiplier: values["BATT_VOLT_MULT"],
        amps_per_volt: values["BATT_AMP_PERVLT"],
        low_voltage: values["BATT_LOW_VOLT"],
        critical_voltage: values["BATT_CRT_VOLT"],
        voltage_v: session.battery_voltage_v,
        current_a: session.battery_current_a,
        remaining_percent: session.battery_remaining_percent,
    }
}

const BATTERY_PARAMS: &[&str] = &[
    "BATT_MONITOR",
    "BATT_CAPACITY",
    "BATT_VOLT_MULT",
    "BATT_AMP_PERVLT",
    "BATT_LOW_VOLT",
    "BATT_CRT_VOLT",
];

#[tauri::command]
pub fn battery_configuration(
    service: State<'_, CalibrationService>,
) -> Result<BatteryConfiguration, String> {
    let values = read_parameters(&service, BATTERY_PARAMS)?;
    Ok(battery_from_values(&service.0.lock().unwrap(), &values))
}

#[tauri::command]
pub fn set_battery_configuration(
    request: BatteryUpdate,
    service: State<'_, CalibrationService>,
) -> Result<BatteryConfiguration, String> {
    if request.capacity_mah <= 0.0
        || request.voltage_multiplier <= 0.0
        || request.amps_per_volt < 0.0
        || request.low_voltage <= request.critical_voltage
        || request.critical_voltage <= 0.0
    {
        return Err(
            "INVALID_BATTERY_CONFIGURATION: check capacity, scaling, and voltage thresholds."
                .into(),
        );
    }
    write_parameters(
        &service,
        BTreeMap::from([
            ("BATT_MONITOR".into(), request.monitor_type as f32),
            ("BATT_CAPACITY".into(), request.capacity_mah),
            ("BATT_VOLT_MULT".into(), request.voltage_multiplier),
            ("BATT_AMP_PERVLT".into(), request.amps_per_volt),
            ("BATT_LOW_VOLT".into(), request.low_voltage),
            ("BATT_CRT_VOLT".into(), request.critical_voltage),
        ]),
    )?;
    battery_configuration(service)
}

#[tauri::command]
pub fn calibrate_battery_voltage(
    request: BatteryVoltageCalibration,
    service: State<'_, CalibrationService>,
) -> Result<BatteryConfiguration, String> {
    let telemetry_voltage = service
        .0
        .lock()
        .unwrap()
        .battery_voltage_v
        .ok_or("NO_BATTERY_TELEMETRY: wait for SYS_STATUS battery voltage.")?;
    if !(1.0..=100.0).contains(&request.measured_voltage) || telemetry_voltage <= 0.0 {
        return Err("INVALID_MEASURED_VOLTAGE".into());
    }
    let values = read_parameters(&service, BATTERY_PARAMS)?;
    let multiplier = values["BATT_VOLT_MULT"] * request.measured_voltage / telemetry_voltage;
    write_parameters(
        &service,
        BTreeMap::from([("BATT_VOLT_MULT".into(), multiplier)]),
    )?;
    battery_configuration(service)
}

#[tauri::command]
pub fn run_motor_test(
    request: MotorTestRequest,
    service: State<'_, CalibrationService>,
) -> Result<MotorTestResponse, String> {
    if !request.propellers_removed {
        return Err("PROPELLERS_PRESENT: remove all propellers before motor testing.".into());
    }
    if !(1..=32).contains(&request.motor_number)
        || !(1.0..=15.0).contains(&request.throttle_percent)
        || !(0.1..=5.0).contains(&request.duration_seconds)
    {
        return Err("INVALID_MOTOR_TEST: motor 1-32, throttle 1-15%, and duration 0.1-5 seconds are allowed.".into());
    }
    let receiver = {
        let session = service.0.lock().unwrap();
        if session.status.firmware != Some(Firmware::ArduPilot) {
            return Err("UNSUPPORTED_FEATURE: motor test currently requires ArduPilot.".into());
        }
        if session.status.armed != Some(false) {
            return Err("VEHICLE_ARMED: disarm before motor testing.".into());
        }
        let (reply, receiver) = mpsc::channel();
        session
            .sender
            .as_ref()
            .ok_or("Vehicle connection is unavailable.")?
            .send(WorkerCommand::MotorTest {
                motor_number: request.motor_number,
                throttle_percent: request.throttle_percent,
                duration_seconds: request.duration_seconds,
                reply,
            })
            .map_err(|_| "Vehicle connection was lost.".to_string())?;
        receiver
    };
    receiver
        .recv_timeout(Duration::from_secs(8))
        .map_err(|_| "Timed out waiting for motor-test acknowledgement.".to_string())?
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
        u8,
    )> = None;
    let mut pending_frame_write: Option<PendingFrameWrite> = None;
    let mut pending_parameter_write: Option<PendingParameterWrite> = None;
    let mut pending_parameter_read: Option<PendingParameterRead> = None;
    let mut pending_motor_test: Option<(
        u8,
        f32,
        f32,
        Instant,
        mpsc::Sender<Result<MotorTestResponse, String>>,
    )> = None;
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
                    if pending_frame_read.is_some()
                        || pending_frame_write.is_some()
                        || pending_parameter_read.is_some()
                        || pending_parameter_write.is_some()
                    {
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
                            attempts: 0,
                            reply,
                        });
                    }
                }
                WorkerCommand::ReadFrame(reply) => {
                    if pending_frame_read.is_some()
                        || pending_frame_write.is_some()
                        || pending_parameter_read.is_some()
                        || pending_parameter_write.is_some()
                    {
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
                        pending_frame_read = Some((reply, None, None, Instant::now(), 0));
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
                WorkerCommand::RequestRadioStream => {
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
                        let request = mav::COMMAND_LONG_DATA {
                            target_system: system_id,
                            target_component: component_id,
                            command: mav::MavCmd::MAV_CMD_SET_MESSAGE_INTERVAL,
                            confirmation: 0,
                            param1: 65.0,
                            param2: 100_000.0,
                            param3: 0.0,
                            param4: 0.0,
                            param5: 0.0,
                            param6: 0.0,
                            param7: 0.0,
                        };
                        let _ = connection.send(&header, &mav::MavMessage::COMMAND_LONG(request));
                    }
                }
                WorkerCommand::ReadParameters { names, reply } => {
                    if pending_parameter_read.is_some()
                        || pending_parameter_write.is_some()
                        || pending_frame_read.is_some()
                        || pending_frame_write.is_some()
                    {
                        let _ = reply.send(Err(
                            "Another parameter operation is already in progress.".into(),
                        ));
                        continue;
                    }
                    let status = service.0.lock().unwrap().status.clone();
                    let (Some(system_id), Some(component_id)) =
                        (status.system_id, status.component_id)
                    else {
                        let _ = reply.send(Err("Vehicle identity is unavailable.".into()));
                        continue;
                    };
                    let mut error = None;
                    for name in &names {
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
                        if let Err(send_error) =
                            connection.send(&header, &mav::MavMessage::PARAM_REQUEST_READ(request))
                        {
                            error = Some(format!("Could not request {name}: {send_error}"));
                            break;
                        }
                    }
                    if let Some(error) = error {
                        let _ = reply.send(Err(error));
                    } else {
                        pending_parameter_read = Some(PendingParameterRead {
                            names,
                            values: BTreeMap::new(),
                            started: Instant::now(),
                            attempts: 0,
                            reply,
                        });
                    }
                }
                WorkerCommand::WriteParameters { parameters, reply } => {
                    if pending_parameter_write.is_some()
                        || pending_parameter_read.is_some()
                        || pending_frame_read.is_some()
                        || pending_frame_write.is_some()
                    {
                        let _ = reply.send(Err(
                            "Another parameter operation is already in progress.".into(),
                        ));
                        continue;
                    }
                    let status = service.0.lock().unwrap().status.clone();
                    let (Some(system_id), Some(component_id)) =
                        (status.system_id, status.component_id)
                    else {
                        let _ = reply.send(Err("Vehicle identity is unavailable.".into()));
                        continue;
                    };
                    let mut error = None;
                    for (name, value) in &parameters {
                        sequence = sequence.wrapping_add(1);
                        let header = MavHeader {
                            system_id: 255,
                            component_id: 190,
                            sequence,
                        };
                        let set = mav::PARAM_SET_DATA {
                            target_system: system_id,
                            target_component: component_id,
                            param_id: parameter_id(name).into(),
                            param_value: *value,
                            param_type: mav::MavParamType::MAV_PARAM_TYPE_REAL32,
                        };
                        if let Err(send_error) =
                            connection.send(&header, &mav::MavMessage::PARAM_SET(set))
                        {
                            error = Some(format!("Could not write {name}: {send_error}"));
                            break;
                        }
                    }
                    if let Some(error) = error {
                        let _ = reply.send(Err(error));
                    } else {
                        pending_parameter_write = Some(PendingParameterWrite {
                            expected: parameters,
                            seen: BTreeMap::new(),
                            verifying: false,
                            started: Instant::now(),
                            attempts: 0,
                            reply,
                        });
                    }
                }
                WorkerCommand::MotorTest {
                    motor_number,
                    throttle_percent,
                    duration_seconds,
                    reply,
                } => {
                    if pending_motor_test.is_some() {
                        let _ = reply.send(Err("A motor test request is already pending.".into()));
                        continue;
                    }
                    let status = service.0.lock().unwrap().status.clone();
                    let (Some(system_id), Some(component_id)) =
                        (status.system_id, status.component_id)
                    else {
                        let _ = reply.send(Err("Vehicle identity is unavailable.".into()));
                        continue;
                    };
                    sequence = sequence.wrapping_add(1);
                    let header = MavHeader {
                        system_id: 255,
                        component_id: 190,
                        sequence,
                    };
                    let command = motor_test_command(
                        system_id,
                        component_id,
                        motor_number,
                        throttle_percent,
                        duration_seconds,
                    );
                    match connection.send(&header, &mav::MavMessage::COMMAND_LONG(command)) {
                        Ok(_) => {
                            pending_motor_test = Some((
                                motor_number,
                                throttle_percent,
                                duration_seconds,
                                Instant::now(),
                                reply,
                            ))
                        }
                        Err(error) => {
                            let _ = reply.send(Err(format!("Could not send motor test: {error}")));
                        }
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
                        s.active_mode = Some(heartbeat.custom_mode);
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
                    if ack.command == mav::MavCmd::MAV_CMD_DO_MOTOR_TEST {
                        if let Some((motor, throttle, duration, _, reply)) =
                            pending_motor_test.take()
                        {
                            if ack.result == mav::MavResult::MAV_RESULT_ACCEPTED
                                || ack.result == mav::MavResult::MAV_RESULT_IN_PROGRESS
                            {
                                let _ = reply.send(Ok(MotorTestResponse {
                                    accepted: true,
                                    motor_number: motor,
                                    throttle_percent: throttle,
                                    duration_seconds: duration,
                                    message: "Flight controller accepted the bounded motor test"
                                        .into(),
                                }));
                            } else {
                                let _ = reply.send(Err(format!(
                                    "Motor test rejected by flight controller: {:?}",
                                    ack.result
                                )));
                            }
                        }
                        continue;
                    }
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
                    if let Some(mut read) = pending_parameter_read.take() {
                        if read.names.iter().any(|name| name == parameter_name) {
                            read.values
                                .insert(parameter_name.to_string(), parameter.param_value);
                        }
                        if read.values.len() == read.names.len() {
                            let _ = read.reply.send(Ok(read.values));
                        } else {
                            pending_parameter_read = Some(read);
                        }
                    }
                    if let Some(mut write) = pending_parameter_write.take() {
                        if write.expected.contains_key(parameter_name) {
                            write
                                .seen
                                .insert(parameter_name.to_string(), parameter.param_value);
                        }
                        if write.seen.len() == write.expected.len() {
                            let mismatch = write.expected.iter().find(|(name, expected)| {
                                write
                                    .seen
                                    .get(*name)
                                    .is_none_or(|actual| (*actual - **expected).abs() >= 0.5)
                            });
                            if let Some((name, expected)) = mismatch {
                                let actual = write.seen.get(name).copied();
                                let _ = write.reply.send(Err(format!(
                                    "PARAMETER_VERIFICATION_FAILED: {name} expected {expected}, received {actual:?}."
                                )));
                            } else if write.verifying {
                                let _ = write.reply.send(Ok(()));
                            } else {
                                let status = service.0.lock().unwrap().status.clone();
                                if let (Some(system_id), Some(component_id)) =
                                    (status.system_id, status.component_id)
                                {
                                    let mut error = None;
                                    for name in write.expected.keys() {
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
                                        if let Err(send_error) = connection.send(
                                            &header,
                                            &mav::MavMessage::PARAM_REQUEST_READ(request),
                                        ) {
                                            error = Some(format!(
                                                "Could not read back {name}: {send_error}"
                                            ));
                                            break;
                                        }
                                    }
                                    if let Some(error) = error {
                                        let _ = write.reply.send(Err(error));
                                    } else {
                                        write.seen.clear();
                                        write.verifying = true;
                                        write.started = Instant::now();
                                        write.attempts = 0;
                                        pending_parameter_write = Some(write);
                                    }
                                }
                            }
                        } else {
                            pending_parameter_write = Some(write);
                        }
                    }
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
                                        write.attempts = 0;
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
                    if let Some((_, class_value, type_value, _, _)) = &mut pending_frame_read {
                        match parameter_name {
                            "FRAME_CLASS" => *class_value = Some(parameter.param_value),
                            "FRAME_TYPE" => *type_value = Some(parameter.param_value),
                            _ => {}
                        }
                    }
                    let completed = pending_frame_read.as_ref().and_then(
                        |(_, class_value, type_value, _, _)| match (*class_value, *type_value) {
                            (Some(class_value), Some(type_value)) => {
                                Some(resolve_frame_configuration(class_value, type_value))
                            }
                            _ => None,
                        },
                    );
                    if let Some(configuration) = completed {
                        if let Some((reply, _, _, _, _)) = pending_frame_read.take() {
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
                mav::MavMessage::RC_CHANNELS(channels) => {
                    let values = [
                        channels.chan1_raw,
                        channels.chan2_raw,
                        channels.chan3_raw,
                        channels.chan4_raw,
                        channels.chan5_raw,
                        channels.chan6_raw,
                        channels.chan7_raw,
                        channels.chan8_raw,
                        channels.chan9_raw,
                        channels.chan10_raw,
                        channels.chan11_raw,
                        channels.chan12_raw,
                        channels.chan13_raw,
                        channels.chan14_raw,
                        channels.chan15_raw,
                        channels.chan16_raw,
                        channels.chan17_raw,
                        channels.chan18_raw,
                    ];
                    change(&app, &service, generation, |s| {
                        for (index, value) in values.into_iter().enumerate() {
                            let channel_number = (index + 1) as u8;
                            if channel_number > channels.chancount
                                || value == 0
                                || value == u16::MAX
                            {
                                continue;
                            }
                            if let Some(channel) = s
                                .radio_channels
                                .iter_mut()
                                .find(|channel| channel.channel == channel_number)
                            {
                                channel.value_us = value;
                                if s.radio_active && s.radio_capture_min_max {
                                    channel.minimum_us = channel.minimum_us.min(value);
                                    channel.maximum_us = channel.maximum_us.max(value);
                                }
                                channel.range_valid =
                                    radio_range_valid(channel.minimum_us, channel.maximum_us);
                            } else {
                                s.radio_channels.push(RadioChannelStatus {
                                    channel: channel_number,
                                    value_us: value,
                                    minimum_us: value,
                                    maximum_us: value,
                                    trim_us: value,
                                    range_valid: false,
                                });
                            }
                        }
                        s.radio_channels.sort_by_key(|channel| channel.channel);
                    });
                }
                mav::MavMessage::SYS_STATUS(status) => {
                    change(&app, &service, generation, |s| {
                        if s.status.system_id == Some(header.system_id) {
                            s.battery_voltage_v = (status.voltage_battery != u16::MAX)
                                .then_some(status.voltage_battery as f32 / 1000.0);
                            s.battery_current_a = (status.current_battery != -1)
                                .then_some(status.current_battery as f32 / 100.0);
                            s.battery_remaining_percent =
                                (status.battery_remaining >= 0).then_some(status.battery_remaining);
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
            .is_some_and(|(_, _, _, started, _)| started.elapsed() > Duration::from_millis(900))
        {
            if let Some((reply, class_value, type_value, _, attempts)) = pending_frame_read.take() {
                if attempts >= 4 {
                    let missing = match (class_value, type_value) {
                        (None, None) => "FRAME_CLASS and FRAME_TYPE",
                        (None, Some(_)) => "FRAME_CLASS",
                        (Some(_), None) => "FRAME_TYPE",
                        (Some(_), Some(_)) => "no parameters",
                    };
                    let _ = reply.send(Err(format!(
                        "Timed out waiting for {missing} from the flight controller after retries."
                    )));
                } else {
                    let status = service.0.lock().unwrap().status.clone();
                    if let (Some(system_id), Some(component_id)) =
                        (status.system_id, status.component_id)
                    {
                        for name in [
                            class_value.is_none().then_some("FRAME_CLASS"),
                            type_value.is_none().then_some("FRAME_TYPE"),
                        ]
                        .into_iter()
                        .flatten()
                        {
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
                            let _ = connection
                                .send(&header, &mav::MavMessage::PARAM_REQUEST_READ(request));
                        }
                    }
                    pending_frame_read =
                        Some((reply, class_value, type_value, Instant::now(), attempts + 1));
                }
            }
        }
        if pending_frame_write
            .as_ref()
            .is_some_and(|write| write.started.elapsed() > Duration::from_millis(900))
        {
            if let Some(mut write) = pending_frame_write.take() {
                if write.attempts >= 4 {
                    let phase = if write.verifying {
                        "readback verification"
                    } else {
                        "parameter write acknowledgement"
                    };
                    let _ = write.reply.send(Err(format!(
                        "Timed out waiting for frame {phase} from the flight controller after retries."
                    )));
                } else {
                    let status = service.0.lock().unwrap().status.clone();
                    if let (Some(system_id), Some(component_id)) =
                        (status.system_id, status.component_id)
                    {
                        let missing = [
                            ("FRAME_CLASS", write.class_value, write.class_seen.is_none()),
                            ("FRAME_TYPE", write.type_value, write.type_seen.is_none()),
                        ];
                        let mut send_error = None;
                        for (name, value, is_missing) in missing {
                            if !is_missing {
                                continue;
                            }
                            sequence = sequence.wrapping_add(1);
                            let header = MavHeader {
                                system_id: 255,
                                component_id: 190,
                                sequence,
                            };
                            let result = if write.verifying {
                                connection.send(
                                    &header,
                                    &mav::MavMessage::PARAM_REQUEST_READ(
                                        mav::PARAM_REQUEST_READ_DATA {
                                            target_system: system_id,
                                            target_component: component_id,
                                            param_id: parameter_id(name).into(),
                                            param_index: -1,
                                        },
                                    ),
                                )
                            } else {
                                connection.send(
                                    &header,
                                    &mav::MavMessage::PARAM_SET(mav::PARAM_SET_DATA {
                                        target_system: system_id,
                                        target_component: component_id,
                                        param_id: parameter_id(name).into(),
                                        param_value: value,
                                        param_type: mav::MavParamType::MAV_PARAM_TYPE_REAL32,
                                    }),
                                )
                            };
                            if let Err(error) = result {
                                send_error = Some(format!("Could not retry {name}: {error}"));
                                break;
                            }
                        }
                        if let Some(error) = send_error {
                            let _ = write.reply.send(Err(error));
                        } else {
                            write.attempts += 1;
                            write.started = Instant::now();
                            pending_frame_write = Some(write);
                        }
                    } else {
                        let _ = write.reply.send(Err(
                            "Vehicle identity was lost during frame parameter retries.".into(),
                        ));
                    }
                }
            }
        }
        if pending_parameter_write
            .as_ref()
            .is_some_and(|write| write.started.elapsed() > Duration::from_millis(900))
        {
            if let Some(mut write) = pending_parameter_write.take() {
                let missing = write
                    .expected
                    .keys()
                    .filter(|name| !write.seen.contains_key(*name))
                    .cloned()
                    .collect::<Vec<_>>();
                if write.attempts >= 4 {
                    let _ = write.reply.send(Err(format!(
                        "Timed out waiting for parameter {} after retries: {}",
                        if write.verifying {
                            "readback"
                        } else {
                            "write acknowledgement"
                        },
                        missing.join(", ")
                    )));
                } else {
                    let status = service.0.lock().unwrap().status.clone();
                    if let (Some(system_id), Some(component_id)) =
                        (status.system_id, status.component_id)
                    {
                        let mut send_error = None;
                        for name in missing {
                            sequence = sequence.wrapping_add(1);
                            let header = MavHeader {
                                system_id: 255,
                                component_id: 190,
                                sequence,
                            };
                            let result = if write.verifying {
                                connection.send(
                                    &header,
                                    &mav::MavMessage::PARAM_REQUEST_READ(
                                        mav::PARAM_REQUEST_READ_DATA {
                                            target_system: system_id,
                                            target_component: component_id,
                                            param_id: parameter_id(&name).into(),
                                            param_index: -1,
                                        },
                                    ),
                                )
                            } else {
                                connection.send(
                                    &header,
                                    &mav::MavMessage::PARAM_SET(mav::PARAM_SET_DATA {
                                        target_system: system_id,
                                        target_component: component_id,
                                        param_id: parameter_id(&name).into(),
                                        param_value: write.expected[&name],
                                        param_type: mav::MavParamType::MAV_PARAM_TYPE_REAL32,
                                    }),
                                )
                            };
                            if let Err(error) = result {
                                send_error = Some(format!("Could not retry {name}: {error}"));
                                break;
                            }
                        }
                        if let Some(error) = send_error {
                            let _ = write.reply.send(Err(error));
                        } else {
                            write.attempts += 1;
                            write.started = Instant::now();
                            pending_parameter_write = Some(write);
                        }
                    } else {
                        let _ = write.reply.send(Err(
                            "Vehicle identity was lost during parameter retries.".into(),
                        ));
                    }
                }
            }
        }
        if pending_parameter_read
            .as_ref()
            .is_some_and(|read| read.started.elapsed() > Duration::from_millis(900))
        {
            if let Some(mut read) = pending_parameter_read.take() {
                let missing = read
                    .names
                    .iter()
                    .filter(|name| !read.values.contains_key(*name))
                    .cloned()
                    .collect::<Vec<_>>();
                if read.attempts >= 4 {
                    let _ = read.reply.send(Err(format!(
                        "Timed out reading parameters after retries: {}",
                        missing.join(", ")
                    )));
                } else {
                    let status = service.0.lock().unwrap().status.clone();
                    if let (Some(system_id), Some(component_id)) =
                        (status.system_id, status.component_id)
                    {
                        let mut send_error = None;
                        for name in missing {
                            sequence = sequence.wrapping_add(1);
                            let header = MavHeader {
                                system_id: 255,
                                component_id: 190,
                                sequence,
                            };
                            let request = mav::PARAM_REQUEST_READ_DATA {
                                target_system: system_id,
                                target_component: component_id,
                                param_id: parameter_id(&name).into(),
                                param_index: -1,
                            };
                            if let Err(error) = connection
                                .send(&header, &mav::MavMessage::PARAM_REQUEST_READ(request))
                            {
                                send_error = Some(format!("Could not retry {name}: {error}"));
                                break;
                            }
                        }
                        if let Some(error) = send_error {
                            let _ = read.reply.send(Err(error));
                        } else {
                            read.attempts += 1;
                            read.started = Instant::now();
                            pending_parameter_read = Some(read);
                        }
                    } else {
                        let _ = read.reply.send(Err(
                            "Vehicle identity was lost during parameter retries.".into(),
                        ));
                    }
                }
            }
        }
        if pending_motor_test
            .as_ref()
            .is_some_and(|(_, _, _, started, _)| started.elapsed() > Duration::from_secs(6))
        {
            if let Some((_, _, _, _, reply)) = pending_motor_test.take() {
                let _ = reply.send(Err(
                    "Motor-test result is unknown because no COMMAND_ACK was received.".into(),
                ));
            }
        }
        // A 115200-baud ArduPilot link can deliver substantially more than 50
        // MAVLink packets per second. Sleeping 20 ms after every single receive
        // allowed telemetry to build an unbounded backlog, hiding command ACKs
        // and interactive accelerometer position requests behind older packets.
        thread::sleep(Duration::from_millis(1));
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
    fn radio_ranges_require_meaningful_travel() {
        assert!(radio_range_valid(1000, 2000));
        assert!(radio_range_valid(1100, 1500));
        assert!(!radio_range_valid(1300, 1699));
        assert!(!radio_range_valid(1800, 1200));
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
    fn completed_or_cancelled_esc_workflow_cannot_advance() {
        assert!(next_esc_step(ESC_STEPS.len(), false, ESC_STEPS[4].0).is_err());
        assert!(next_esc_step(2, false, ESC_STEPS[2].0).is_err());
    }

    #[test]
    fn motor_test_uses_percent_throttle_and_bounded_duration() {
        let command = motor_test_command(1, 1, 2, 5.0, 1.0);
        assert_eq!(command.command, mav::MavCmd::MAV_CMD_DO_MOTOR_TEST);
        assert_eq!(command.param1, 2.0);
        assert_eq!(command.param2, 0.0);
        assert_eq!(command.param3, 5.0);
        assert_eq!(command.param4, 1.0);
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
