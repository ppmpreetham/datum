use crate::server;
use ::mavlink::{
    connect,
    dialects::ardupilotmega::{
        self, MavAutopilot, MavCmd, MavMessage, MavResult, MavSysStatusSensor, COMMAND_LONG_DATA,
    },
    MavConnection, MavHeader, MavlinkVersion,
};
use serde::Serialize;
use serialport::{SerialPortInfo, SerialPortType};
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const DEFAULT_BAUD_RATE: u32 = 115_200;
const DEFAULT_BAUD_RATES: [u32; 2] = [115_200, 57_600];
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const AUTO_CONNECT_TIMEOUT: Duration = Duration::from_millis(1800);
const HEARTBEAT_LOST_AFTER: Duration = Duration::from_secs(5);
const WORKER_IDLE_DELAY: Duration = Duration::from_millis(10);
const MAX_PROGRESS_EVENTS: usize = 200;
const GCS_SYSTEM_ID: u8 = 255;
const GCS_COMPONENT_ID: u8 = 190;

type MavlinkConnection = ::mavlink::Connection<MavMessage>;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SerialPortSummary {
    pub port_name: String,
    pub port_type: String,
    pub is_likely_flight_controller: bool,
    pub usb_vendor_id: Option<u16>,
    pub usb_product_id: Option<u16>,
    pub serial_number: Option<String>,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ConnectionPhase {
    Disconnected,
    Connecting,
    Ready,
    HeartbeatLost,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FlightStack {
    ArduPilot,
    Px4,
    Generic,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DroneStatus {
    pub connected: bool,
    pub connection_phase: ConnectionPhase,
    pub port_name: Option<String>,
    pub baud_rate: Option<u32>,
    pub flight_stack: Option<FlightStack>,
    pub autopilot_code: Option<u32>,
    pub system_id: Option<u8>,
    pub component_id: Option<u8>,
    pub last_heartbeat_ms: Option<u128>,
    pub last_error: Option<String>,
}

impl Default for DroneStatus {
    fn default() -> Self {
        Self {
            connected: false,
            connection_phase: ConnectionPhase::Disconnected,
            port_name: None,
            baud_rate: None,
            flight_stack: None,
            autopilot_code: None,
            system_id: None,
            component_id: None,
            last_heartbeat_ms: None,
            last_error: None,
        }
    }
}

#[derive(Default)]
pub struct DroneState {
    status: Arc<Mutex<DroneStatus>>,
    telemetry: Arc<Mutex<HardwareTelemetry>>,
    calibration_progress: Arc<Mutex<VecDeque<CalibrationProgressEvent>>>,
    calibration_session: Arc<Mutex<Option<CalibrationSession>>>,
    connection: Mutex<Option<Connection>>,
}

struct Connection {
    stop: Arc<AtomicBool>,
    worker: JoinHandle<()>,
    mavlink: Arc<MavlinkConnection>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoConnectResult {
    pub port_name: String,
    pub baud_rate: u32,
    pub status: DroneStatus,
    pub attempts: Vec<ConnectionAttempt>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionAttempt {
    pub port_name: String,
    pub baud_rate: u32,
    pub successful: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MavlinkCommandReceipt {
    pub command: u32,
    pub target_system: u8,
    pub target_component: u8,
    pub sent_at_ms: u128,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareTelemetry {
    pub sensor_health: Option<SensorHealth>,
    pub imu: Option<ImuReading>,
    pub attitude: Option<AttitudeReading>,
    pub gps: Option<GpsReading>,
    pub battery: Option<BatteryReading>,
    pub last_message_ms: Option<u128>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SensorHealth {
    pub sensors_present: u32,
    pub sensors_enabled: u32,
    pub sensors_health: u32,
    pub accelerometer_healthy: bool,
    pub gyroscope_healthy: bool,
    pub compass_healthy: bool,
    pub barometer_healthy: bool,
    pub gps_healthy: bool,
    pub last_update_ms: u128,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImuReading {
    pub time_boot_ms: u32,
    pub xacc_mg: i16,
    pub yacc_mg: i16,
    pub zacc_mg: i16,
    pub xgyro_mrad_s: i16,
    pub ygyro_mrad_s: i16,
    pub zgyro_mrad_s: i16,
    pub xmag_mgauss: i16,
    pub ymag_mgauss: i16,
    pub zmag_mgauss: i16,
    pub last_update_ms: u128,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttitudeReading {
    pub time_boot_ms: u32,
    pub roll_rad: f32,
    pub pitch_rad: f32,
    pub yaw_rad: f32,
    pub rollspeed_rad_s: f32,
    pub pitchspeed_rad_s: f32,
    pub yawspeed_rad_s: f32,
    pub last_update_ms: u128,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpsReading {
    pub fix_type: u8,
    pub satellites_visible: u8,
    pub latitude_e7: i32,
    pub longitude_e7: i32,
    pub altitude_mm: i32,
    pub horizontal_accuracy_cm: u16,
    pub vertical_accuracy_cm: u16,
    pub velocity_cm_s: u16,
    pub course_over_ground_cdeg: u16,
    pub last_update_ms: u128,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatteryReading {
    pub voltage_mv: Option<u16>,
    pub current_ca: Option<i16>,
    pub remaining_percent: Option<i8>,
    pub last_update_ms: u128,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TelemetrySubscriptionReceipt {
    pub requests: Vec<MavlinkCommandReceipt>,
    pub note: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationProgressEvent {
    pub event_type: String,
    pub message: String,
    pub severity: Option<u8>,
    pub command: Option<u32>,
    pub result: Option<u8>,
    pub progress: Option<u8>,
    pub received_at_ms: u128,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CalibrationState {
    CommandSent,
    InProgress,
    AwaitingUserAction,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompassResult {
    pub compass_id: u8,
    pub status: String,
    pub fitness: f32,
    pub autosaved: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationSession {
    pub calibration: String,
    pub flight_stack: FlightStack,
    pub state: CalibrationState,
    pub progress_percent: Option<u8>,
    pub instruction: String,
    pub command: u32,
    pub started_at_ms: u128,
    pub updated_at_ms: u128,
    pub error: Option<String>,
    pub compass_results: Vec<CompassResult>,
    #[serde(skip)]
    health_went_unhealthy: bool,
}

#[tauri::command]
pub fn list_serial_ports() -> Result<Vec<SerialPortSummary>, String> {
    serialport::available_ports()
        .map_err(|error| format!("Failed to list serial ports: {error}"))
        .map(|ports| ports.into_iter().map(port_summary).collect())
}

#[tauri::command]
pub fn connect_drone(
    state: tauri::State<'_, DroneState>,
    port_name: String,
    baud_rate: Option<u32>,
) -> Result<DroneStatus, String> {
    connect_verified(
        &state,
        port_name,
        baud_rate.unwrap_or(DEFAULT_BAUD_RATE),
        CONNECT_TIMEOUT,
    )
}

#[tauri::command]
pub fn auto_connect_drone(
    state: tauri::State<'_, DroneState>,
    baud_rates: Option<Vec<u32>>,
) -> Result<AutoConnectResult, String> {
    ensure_no_active_connection(&state)?;
    let mut ports = serialport::available_ports()
        .map_err(|error| format!("Failed to list serial ports: {error}"))?;
    ports.sort_by_key(|port| !is_likely_flight_controller(port));
    let ports: Vec<_> = ports
        .into_iter()
        .filter(is_likely_flight_controller)
        .collect();

    if ports.is_empty() {
        return Err("No likely USB flight-controller serial ports were found. Select a port manually with list_serial_ports and connect_drone.".to_string());
    }

    let baud_rates = sanitize_baud_rates(baud_rates)?;
    let mut attempts = Vec::new();
    for port in ports {
        for baud_rate in &baud_rates {
            match connect_verified(
                &state,
                port.port_name.clone(),
                *baud_rate,
                AUTO_CONNECT_TIMEOUT,
            ) {
                Ok(status) => {
                    attempts.push(ConnectionAttempt {
                        port_name: port.port_name.clone(),
                        baud_rate: *baud_rate,
                        successful: true,
                        error: None,
                    });
                    return Ok(AutoConnectResult {
                        port_name: port.port_name,
                        baud_rate: *baud_rate,
                        status,
                        attempts,
                    });
                }
                Err(error) => attempts.push(ConnectionAttempt {
                    port_name: port.port_name.clone(),
                    baud_rate: *baud_rate,
                    successful: false,
                    error: Some(error),
                }),
            }
        }
    }

    let summary = attempts
        .iter()
        .map(|attempt| format!("{}@{}", attempt.port_name, attempt.baud_rate))
        .collect::<Vec<_>>()
        .join(", ");
    Err(format!(
        "No ArduPilot or PX4 heartbeat was found. Tried: {summary}"
    ))
}

#[tauri::command]
pub fn disconnect_drone(state: tauri::State<'_, DroneState>) -> Result<DroneStatus, String> {
    stop_active_connection(&state)?;
    set_status(&state.status, |status| *status = DroneStatus::default())?;
    reset_runtime_data(&state)?;
    get_status_snapshot(&state.status)
}

#[tauri::command]
pub fn get_drone_status(state: tauri::State<'_, DroneState>) -> Result<DroneStatus, String> {
    get_status_snapshot(&state.status)
}

#[tauri::command]
pub fn get_hardware_telemetry(
    state: tauri::State<'_, DroneState>,
) -> Result<HardwareTelemetry, String> {
    state
        .telemetry
        .lock()
        .map(|value| value.clone())
        .map_err(|_| "Hardware telemetry lock is poisoned".to_string())
}

#[tauri::command]
pub fn get_calibration_progress(
    state: tauri::State<'_, DroneState>,
) -> Result<Vec<CalibrationProgressEvent>, String> {
    state
        .calibration_progress
        .lock()
        .map(|events| events.iter().cloned().collect())
        .map_err(|_| "Calibration progress lock is poisoned".to_string())
}

#[tauri::command]
pub fn get_current_calibration(
    state: tauri::State<'_, DroneState>,
) -> Result<Option<CalibrationSession>, String> {
    state
        .calibration_session
        .lock()
        .map(|session| session.clone())
        .map_err(|_| "Calibration session lock is poisoned".to_string())
}

#[tauri::command]
pub fn request_hardware_telemetry(
    state: tauri::State<'_, DroneState>,
) -> Result<TelemetrySubscriptionReceipt, String> {
    let requests = [
        (1_u32, 1_000_000_u32),
        (24, 1_000_000),
        (26, 200_000),
        (30, 50_000),
    ]
    .into_iter()
    .map(|(message_id, interval_us)| {
        send_command_long(
            &state,
            MavCmd::MAV_CMD_SET_MESSAGE_INTERVAL,
            [
                message_id as f32,
                interval_us as f32,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
            ],
        )
    })
    .collect::<Result<Vec<_>, _>>()?;

    Ok(TelemetrySubscriptionReceipt {
        requests,
        note: "Requested SYS_STATUS and GPS_RAW_INT at 1 Hz, SCALED_IMU at 5 Hz, and ATTITUDE at 20 Hz. Check calibration progress events for individual command acknowledgements.",
    })
}

pub fn send_command_long(
    state: &DroneState,
    command: MavCmd,
    params: [f32; 7],
) -> Result<MavlinkCommandReceipt, String> {
    let status = get_status_snapshot(&state.status)?;
    if !status.connected || status.connection_phase != ConnectionPhase::Ready {
        return Err("Connect to an ArduPilot or PX4 flight controller and wait for a heartbeat before sending a MAVLink command.".to_string());
    }
    let target_system = status
        .system_id
        .ok_or_else(|| "MAVLink target system is not available.".to_string())?;
    let target_component = status
        .component_id
        .ok_or_else(|| "MAVLink target component is not available.".to_string())?;
    let connection = state
        .connection
        .lock()
        .map_err(|_| "Drone connection lock is poisoned".to_string())?;
    let connection = connection
        .as_ref()
        .ok_or_else(|| "No active MAVLink connection.".to_string())?;
    let message = MavMessage::COMMAND_LONG(COMMAND_LONG_DATA {
        param1: params[0],
        param2: params[1],
        param3: params[2],
        param4: params[3],
        param5: params[4],
        param6: params[5],
        param7: params[6],
        command,
        target_system,
        target_component,
        confirmation: 0,
    });
    connection
        .mavlink
        .send(&gcs_header(), &message)
        .map_err(|error| format!("Failed to send MAVLink command: {error}"))?;
    Ok(MavlinkCommandReceipt {
        command: command as u32,
        target_system,
        target_component,
        sent_at_ms: now_ms(),
    })
}

pub fn flight_stack(state: &DroneState) -> Result<FlightStack, String> {
    get_status_snapshot(&state.status)?
        .flight_stack
        .ok_or_else(|| {
            "The flight stack is unknown because no autopilot heartbeat has been received."
                .to_string()
        })
}

pub fn begin_calibration_session(
    state: &DroneState,
    calibration: &str,
    command: MavCmd,
    instruction: &str,
) -> Result<(), String> {
    let stack = flight_stack(state)?;
    let mut session = state
        .calibration_session
        .lock()
        .map_err(|_| "Calibration session lock is poisoned".to_string())?;
    if let Some(active) = session.as_ref() {
        if matches!(
            active.state,
            CalibrationState::CommandSent
                | CalibrationState::InProgress
                | CalibrationState::AwaitingUserAction
        ) {
            return Err(format!(
                "{} calibration is already active. Cancel or finish it first.",
                active.calibration
            ));
        }
    }
    let now = now_ms();
    *session = Some(CalibrationSession {
        calibration: calibration.to_string(),
        flight_stack: stack,
        state: CalibrationState::CommandSent,
        progress_percent: Some(0),
        instruction: instruction.to_string(),
        command: command as u32,
        started_at_ms: now,
        updated_at_ms: now,
        error: None,
        compass_results: Vec::new(),
        health_went_unhealthy: false,
    });
    Ok(())
}

pub fn fail_calibration_start(state: &DroneState, error: &str) {
    if let Ok(mut guard) = state.calibration_session.lock() {
        if let Some(session) = guard.as_mut() {
            session.state = CalibrationState::Failed;
            session.error = Some(error.to_string());
            session.updated_at_ms = now_ms();
        }
    }
}

pub fn require_calibration_session(
    state: &DroneState,
    expected: &str,
    active_only: bool,
) -> Result<CalibrationState, String> {
    let guard = state
        .calibration_session
        .lock()
        .map_err(|_| "Calibration session lock is poisoned".to_string())?;
    let session = guard
        .as_ref()
        .ok_or_else(|| format!("No {expected} calibration session exists."))?;
    if session.calibration != expected {
        return Err(format!(
            "The current calibration is {}, not {expected}.",
            session.calibration
        ));
    }
    if active_only
        && matches!(
            session.state,
            CalibrationState::Completed | CalibrationState::Failed | CalibrationState::Cancelled
        )
    {
        return Err(format!(
            "The {expected} calibration is no longer active (state: {:?}).",
            session.state
        ));
    }
    Ok(session.state)
}

pub fn update_calibration_instruction(
    state: &DroneState,
    expected: &str,
    instruction: &str,
) -> Result<(), String> {
    require_calibration_session(state, expected, true)?;
    let mut guard = state
        .calibration_session
        .lock()
        .map_err(|_| "Calibration session lock is poisoned".to_string())?;
    let session = guard.as_mut().expect("session was checked above");
    session.state = CalibrationState::InProgress;
    session.instruction = instruction.to_string();
    session.updated_at_ms = now_ms();
    Ok(())
}

fn connect_verified(
    state: &DroneState,
    port_name: String,
    baud_rate: u32,
    timeout: Duration,
) -> Result<DroneStatus, String> {
    if baud_rate == 0 {
        return Err("Baud rate must be greater than zero.".to_string());
    }
    ensure_no_active_connection(state)?;
    set_status(&state.status, |status| {
        *status = DroneStatus {
            connection_phase: ConnectionPhase::Connecting,
            port_name: Some(port_name.clone()),
            baud_rate: Some(baud_rate),
            ..DroneStatus::default()
        };
    })?;

    let address = format!("serial:{port_name}:{baud_rate}");
    let mut link = connect::<MavMessage>(&address)
        .map_err(|error| connection_error(state, &port_name, baud_rate, &error.to_string()))?;
    link.set_protocol_version(MavlinkVersion::V2);
    link.set_allow_recv_any_version(true);
    let link = Arc::new(link);
    let deadline = Instant::now() + timeout;
    let (header, heartbeat) = loop {
        if Instant::now() >= deadline {
            let error = format!(
                "Opened {port_name} at {baud_rate} baud, but no ArduPilot or PX4 heartbeat arrived within {} ms.",
                timeout.as_millis()
            );
            mark_connection_error(state, error.clone());
            return Err(error);
        }
        match link.try_recv() {
            Ok((header, MavMessage::HEARTBEAT(heartbeat)))
                if supported_autopilot(heartbeat.autopilot) =>
            {
                break (header, heartbeat)
            }
            Ok(_) => {}
            Err(_) => thread::sleep(WORKER_IDLE_DELAY),
        }
    };

    reset_runtime_data(state)?;
    apply_heartbeat(
        &state.status,
        &header,
        heartbeat.autopilot,
        Some((&port_name, baud_rate)),
    );
    start_worker(state, link)?;
    get_status_snapshot(&state.status)
}

fn start_worker(state: &DroneState, link: Arc<MavlinkConnection>) -> Result<(), String> {
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = Arc::clone(&stop);
    let worker_link = Arc::clone(&link);
    let status = Arc::clone(&state.status);
    let telemetry = Arc::clone(&state.telemetry);
    let progress = Arc::clone(&state.calibration_progress);
    let session = Arc::clone(&state.calibration_session);
    let worker = thread::spawn(move || {
        let mut last_heartbeat = Instant::now();
        while !worker_stop.load(Ordering::Relaxed) {
            match worker_link.try_recv() {
                Ok((header, message)) => {
                    if let MavMessage::HEARTBEAT(ref heartbeat) = message {
                        if supported_autopilot(heartbeat.autopilot) {
                            last_heartbeat = Instant::now();
                        }
                    }
                    handle_message(&status, &telemetry, &progress, &session, header, message);
                }
                Err(_) => thread::sleep(WORKER_IDLE_DELAY),
            }
            if last_heartbeat.elapsed() > HEARTBEAT_LOST_AFTER {
                let _ = set_status(&status, |value| {
                    value.connected = false;
                    value.connection_phase = ConnectionPhase::HeartbeatLost;
                    value.last_error = Some("MAVLink heartbeat was lost. Check the cable, port, baud rate, and flight-controller power.".to_string());
                });
            }
        }
    });
    *state
        .connection
        .lock()
        .map_err(|_| "Drone connection lock is poisoned".to_string())? = Some(Connection {
        stop,
        worker,
        mavlink: link,
    });
    Ok(())
}

fn handle_message(
    status: &Arc<Mutex<DroneStatus>>,
    telemetry: &Arc<Mutex<HardwareTelemetry>>,
    events: &Arc<Mutex<VecDeque<CalibrationProgressEvent>>>,
    session: &Arc<Mutex<Option<CalibrationSession>>>,
    header: MavHeader,
    message: MavMessage,
) {
    match message {
        MavMessage::HEARTBEAT(data) if supported_autopilot(data.autopilot) => {
            apply_heartbeat(status, &header, data.autopilot, None)
        }
        MavMessage::SYS_STATUS(data) => update_system_status(telemetry, session, data),
        MavMessage::SCALED_IMU(data) => update_imu(telemetry, data),
        MavMessage::ATTITUDE(data) => update_attitude(telemetry, data),
        MavMessage::GPS_RAW_INT(data) => update_gps(telemetry, data),
        MavMessage::COMMAND_ACK(data) => record_command_ack(
            events,
            session,
            data.command,
            data.result,
            (data.progress != u8::MAX).then_some(data.progress),
        ),
        MavMessage::STATUSTEXT(data) => {
            let text = data.text.to_str().unwrap_or("").trim().to_string();
            record_status_text(events, session, data.severity as u8, text);
        }
        MavMessage::MAG_CAL_PROGRESS(data) => record_mag_progress(events, session, data),
        MavMessage::MAG_CAL_REPORT(data) => record_mag_report(events, session, data),
        MavMessage::COMMAND_LONG(data) if data.command == MavCmd::MAV_CMD_ACCELCAL_VEHICLE_POS => {
            record_accel_position(events, session, data.param1)
        }
        _ => {}
    }
}

fn update_system_status(
    telemetry: &Arc<Mutex<HardwareTelemetry>>,
    session: &Arc<Mutex<Option<CalibrationSession>>>,
    data: ardupilotmega::SYS_STATUS_DATA,
) {
    let present = data.onboard_control_sensors_present.bits();
    let enabled = data.onboard_control_sensors_enabled.bits();
    let health = data.onboard_control_sensors_health.bits();
    let gyro_ok = sensor_is_healthy(
        present,
        enabled,
        health,
        MavSysStatusSensor::MAV_SYS_STATUS_SENSOR_3D_GYRO.bits(),
    );
    let now = now_ms();
    if let Ok(mut value) = telemetry.lock() {
        value.sensor_health = Some(SensorHealth {
            sensors_present: present,
            sensors_enabled: enabled,
            sensors_health: health,
            accelerometer_healthy: sensor_is_healthy(
                present,
                enabled,
                health,
                MavSysStatusSensor::MAV_SYS_STATUS_SENSOR_3D_ACCEL.bits(),
            ),
            gyroscope_healthy: gyro_ok,
            compass_healthy: sensor_is_healthy(
                present,
                enabled,
                health,
                MavSysStatusSensor::MAV_SYS_STATUS_SENSOR_3D_MAG.bits(),
            ),
            barometer_healthy: sensor_is_healthy(
                present,
                enabled,
                health,
                MavSysStatusSensor::MAV_SYS_STATUS_SENSOR_ABSOLUTE_PRESSURE.bits(),
            ),
            gps_healthy: sensor_is_healthy(
                present,
                enabled,
                health,
                MavSysStatusSensor::MAV_SYS_STATUS_SENSOR_GPS.bits(),
            ),
            last_update_ms: now,
        });
        value.battery = Some(BatteryReading {
            voltage_mv: (data.voltage_battery != u16::MAX).then_some(data.voltage_battery),
            current_ca: (data.current_battery != -1).then_some(data.current_battery),
            remaining_percent: (data.battery_remaining >= 0).then_some(data.battery_remaining),
            last_update_ms: now,
        });
        value.last_message_ms = Some(now);
    }
    if let Ok(mut guard) = session.lock() {
        if let Some(active) = guard.as_mut() {
            if active.calibration == "gyroscope"
                && !matches!(
                    active.state,
                    CalibrationState::Completed
                        | CalibrationState::Failed
                        | CalibrationState::Cancelled
                )
            {
                if !gyro_ok {
                    active.health_went_unhealthy = true;
                }
                if gyro_ok && active.health_went_unhealthy {
                    active.state = CalibrationState::Completed;
                    active.progress_percent = Some(100);
                    active.instruction = "Gyroscope health was restored after calibration. Keep the vehicle still until normal status messages resume.".to_string();
                    active.updated_at_ms = now;
                }
            }
        }
    }
}

fn update_imu(telemetry: &Arc<Mutex<HardwareTelemetry>>, data: ardupilotmega::SCALED_IMU_DATA) {
    let now = now_ms();
    if let Ok(mut value) = telemetry.lock() {
        value.imu = Some(ImuReading {
            time_boot_ms: data.time_boot_ms,
            xacc_mg: data.xacc,
            yacc_mg: data.yacc,
            zacc_mg: data.zacc,
            xgyro_mrad_s: data.xgyro,
            ygyro_mrad_s: data.ygyro,
            zgyro_mrad_s: data.zgyro,
            xmag_mgauss: data.xmag,
            ymag_mgauss: data.ymag,
            zmag_mgauss: data.zmag,
            last_update_ms: now,
        });
        value.last_message_ms = Some(now);
    }
}

fn update_attitude(telemetry: &Arc<Mutex<HardwareTelemetry>>, data: ardupilotmega::ATTITUDE_DATA) {
    let now = now_ms();
    server::publish_drone_attitude(data.roll, data.pitch, data.yaw);
    if let Ok(mut value) = telemetry.lock() {
        value.attitude = Some(AttitudeReading {
            time_boot_ms: data.time_boot_ms,
            roll_rad: data.roll,
            pitch_rad: data.pitch,
            yaw_rad: data.yaw,
            rollspeed_rad_s: data.rollspeed,
            pitchspeed_rad_s: data.pitchspeed,
            yawspeed_rad_s: data.yawspeed,
            last_update_ms: now,
        });
        value.last_message_ms = Some(now);
    }
}

fn update_gps(telemetry: &Arc<Mutex<HardwareTelemetry>>, data: ardupilotmega::GPS_RAW_INT_DATA) {
    let now = now_ms();
    if let Ok(mut value) = telemetry.lock() {
        value.gps = Some(GpsReading {
            fix_type: data.fix_type as u8,
            satellites_visible: data.satellites_visible,
            latitude_e7: data.lat,
            longitude_e7: data.lon,
            altitude_mm: data.alt,
            horizontal_accuracy_cm: data.eph,
            vertical_accuracy_cm: data.epv,
            velocity_cm_s: data.vel,
            course_over_ground_cdeg: data.cog,
            last_update_ms: now,
        });
        value.last_message_ms = Some(now);
    }
}

fn record_command_ack(
    events: &Arc<Mutex<VecDeque<CalibrationProgressEvent>>>,
    session: &Arc<Mutex<Option<CalibrationSession>>>,
    command: MavCmd,
    result: MavResult,
    progress: Option<u8>,
) {
    let command_id = command as u32;
    let result_id = result as u8;
    push_event(
        events,
        CalibrationProgressEvent {
            event_type: "commandAck".to_string(),
            message: format!("MAVLink command {command_id} returned {result:?}."),
            severity: None,
            command: Some(command_id),
            result: Some(result_id),
            progress,
            received_at_ms: now_ms(),
        },
    );
    if let Ok(mut guard) = session.lock() {
        if let Some(active) = guard.as_mut() {
            if command == MavCmd::MAV_CMD_DO_CANCEL_MAG_CAL
                && result == MavResult::MAV_RESULT_ACCEPTED
            {
                active.state = CalibrationState::Cancelled;
                active.instruction =
                    "Compass calibration was cancelled by the flight controller.".to_string();
                active.updated_at_ms = now_ms();
            } else if command == MavCmd::MAV_CMD_DO_ACCEPT_MAG_CAL
                && result == MavResult::MAV_RESULT_ACCEPTED
            {
                active.state = CalibrationState::Completed;
                active.progress_percent = Some(100);
                active.instruction = "Compass calibration was accepted and saved.".to_string();
                active.updated_at_ms = now_ms();
            } else if active.command == command_id {
                active.updated_at_ms = now_ms();
                if let Some(value) = progress {
                    active.progress_percent = Some(value.min(100));
                }
                match result {
                    MavResult::MAV_RESULT_ACCEPTED | MavResult::MAV_RESULT_IN_PROGRESS => {
                        active.state = CalibrationState::InProgress
                    }
                    MavResult::MAV_RESULT_CANCELLED => active.state = CalibrationState::Cancelled,
                    _ => {
                        active.state = CalibrationState::Failed;
                        active.error =
                            Some(format!("Flight controller rejected command: {result:?}"));
                    }
                }
            }
        }
    }
}

fn record_status_text(
    events: &Arc<Mutex<VecDeque<CalibrationProgressEvent>>>,
    session: &Arc<Mutex<Option<CalibrationSession>>>,
    severity: u8,
    text: String,
) {
    if text.is_empty() {
        return;
    }
    let progress = percentage_in_text(&text);
    push_event(
        events,
        CalibrationProgressEvent {
            event_type: "statusText".to_string(),
            message: text.clone(),
            severity: Some(severity),
            command: None,
            result: None,
            progress,
            received_at_ms: now_ms(),
        },
    );
    if let Ok(mut guard) = session.lock() {
        if let Some(active) = guard.as_mut() {
            let lower = text.to_ascii_lowercase();
            if lower.contains("cal")
                || lower.contains("gyro")
                || lower.contains("accel")
                || lower.contains("mag")
            {
                active.instruction = text;
                active.updated_at_ms = now_ms();
                if let Some(value) = progress {
                    active.progress_percent = Some(value);
                }
                if lower.contains("failed")
                    || lower.contains("failure")
                    || lower.contains("cancelled")
                {
                    active.state = if lower.contains("cancelled") {
                        CalibrationState::Cancelled
                    } else {
                        CalibrationState::Failed
                    };
                    active.error = Some(active.instruction.clone());
                } else if lower.contains("complete")
                    || lower.contains("successful")
                    || lower.contains("success")
                    || lower.contains("done")
                {
                    active.state = CalibrationState::Completed;
                    active.progress_percent = Some(100);
                } else {
                    active.state = CalibrationState::InProgress;
                }
            }
        }
    }
}

fn record_mag_progress(
    events: &Arc<Mutex<VecDeque<CalibrationProgressEvent>>>,
    session: &Arc<Mutex<Option<CalibrationSession>>>,
    data: ardupilotmega::MAG_CAL_PROGRESS_DATA,
) {
    let text = format!(
        "Compass {}: {:?} ({}%).",
        data.compass_id, data.cal_status, data.completion_pct
    );
    push_event(
        events,
        CalibrationProgressEvent {
            event_type: "magCalProgress".to_string(),
            message: text.clone(),
            severity: None,
            command: Some(MavCmd::MAV_CMD_DO_START_MAG_CAL as u32),
            result: None,
            progress: Some(data.completion_pct),
            received_at_ms: now_ms(),
        },
    );
    if let Ok(mut guard) = session.lock() {
        if let Some(active) = guard.as_mut() {
            if active.calibration == "compass" {
                active.state = CalibrationState::InProgress;
                active.progress_percent = Some(data.completion_pct);
                active.instruction = text;
                active.updated_at_ms = now_ms();
            }
        }
    }
}

fn record_mag_report(
    events: &Arc<Mutex<VecDeque<CalibrationProgressEvent>>>,
    session: &Arc<Mutex<Option<CalibrationSession>>>,
    data: ardupilotmega::MAG_CAL_REPORT_DATA,
) {
    let successful = data.cal_status == ardupilotmega::MagCalStatus::MAG_CAL_SUCCESS;
    let text = format!(
        "Compass {} calibration {:?}; fitness {:.2}; autosaved={}.",
        data.compass_id,
        data.cal_status,
        data.fitness,
        data.autosaved != 0
    );
    push_event(
        events,
        CalibrationProgressEvent {
            event_type: "magCalReport".to_string(),
            message: text.clone(),
            severity: None,
            command: Some(MavCmd::MAV_CMD_DO_START_MAG_CAL as u32),
            result: None,
            progress: successful.then_some(100),
            received_at_ms: now_ms(),
        },
    );
    if let Ok(mut guard) = session.lock() {
        if let Some(active) = guard.as_mut() {
            if active.calibration == "compass" {
                active
                    .compass_results
                    .retain(|value| value.compass_id != data.compass_id);
                active.compass_results.push(CompassResult {
                    compass_id: data.compass_id,
                    status: format!("{:?}", data.cal_status),
                    fitness: data.fitness,
                    autosaved: data.autosaved != 0,
                });
                active.state = if successful {
                    CalibrationState::Completed
                } else {
                    CalibrationState::Failed
                };
                active.progress_percent = successful.then_some(100);
                active.instruction = text.clone();
                active.updated_at_ms = now_ms();
                if !successful {
                    active.error = Some(text);
                }
            }
        }
    }
}

fn record_accel_position(
    events: &Arc<Mutex<VecDeque<CalibrationProgressEvent>>>,
    session: &Arc<Mutex<Option<CalibrationSession>>>,
    raw_position: f32,
) {
    let position = raw_position.round() as u32;
    let (instruction, progress, terminal) = match position {
        1 => (
            "Place the vehicle level, then confirm the level position.",
            0,
            None,
        ),
        2 => (
            "Place the vehicle on its left side, then confirm the left position.",
            16,
            None,
        ),
        3 => (
            "Place the vehicle on its right side, then confirm the right position.",
            33,
            None,
        ),
        4 => (
            "Place the vehicle nose-down, then confirm the nose-down position.",
            50,
            None,
        ),
        5 => (
            "Place the vehicle nose-up, then confirm the nose-up position.",
            66,
            None,
        ),
        6 => (
            "Place the vehicle upside-down, then confirm the back position.",
            83,
            None,
        ),
        16_777_215 => (
            "Accelerometer calibration completed successfully.",
            100,
            Some(CalibrationState::Completed),
        ),
        16_777_216 => (
            "Accelerometer calibration failed.",
            0,
            Some(CalibrationState::Failed),
        ),
        _ => return,
    };
    push_event(
        events,
        CalibrationProgressEvent {
            event_type: "accelerometerPosition".to_string(),
            message: instruction.to_string(),
            severity: None,
            command: Some(MavCmd::MAV_CMD_ACCELCAL_VEHICLE_POS as u32),
            result: None,
            progress: Some(progress),
            received_at_ms: now_ms(),
        },
    );
    if let Ok(mut guard) = session.lock() {
        if let Some(active) = guard.as_mut() {
            if active.calibration == "accelerometer" {
                active.state = terminal.unwrap_or(CalibrationState::AwaitingUserAction);
                active.progress_percent = Some(progress);
                active.instruction = instruction.to_string();
                active.updated_at_ms = now_ms();
                if terminal == Some(CalibrationState::Failed) {
                    active.error = Some(instruction.to_string());
                }
            }
        }
    }
}

fn apply_heartbeat(
    status: &Arc<Mutex<DroneStatus>>,
    header: &MavHeader,
    autopilot: MavAutopilot,
    connection: Option<(&str, u32)>,
) {
    let _ = set_status(status, |value| {
        if let Some((port, baud)) = connection {
            value.port_name = Some(port.to_string());
            value.baud_rate = Some(baud);
        }
        value.connected = true;
        value.connection_phase = ConnectionPhase::Ready;
        value.flight_stack = Some(stack_from_autopilot(autopilot));
        value.autopilot_code = Some(autopilot as u32);
        value.system_id = Some(header.system_id);
        value.component_id = Some(header.component_id);
        value.last_heartbeat_ms = Some(now_ms());
        value.last_error = None;
    });
}

fn port_summary(port: SerialPortInfo) -> SerialPortSummary {
    let likely = is_likely_flight_controller(&port);
    let (port_type, vid, pid, serial_number, manufacturer, product) = match port.port_type {
        SerialPortType::UsbPort(info) => (
            format!(
                "USB: {}",
                info.product
                    .clone()
                    .unwrap_or_else(|| "serial device".to_string())
            ),
            Some(info.vid),
            Some(info.pid),
            info.serial_number,
            info.manufacturer,
            info.product,
        ),
        SerialPortType::BluetoothPort => ("Bluetooth".to_string(), None, None, None, None, None),
        SerialPortType::PciPort => ("PCI".to_string(), None, None, None, None, None),
        SerialPortType::Unknown => ("Unknown".to_string(), None, None, None, None, None),
    };
    SerialPortSummary {
        port_name: port.port_name,
        port_type,
        is_likely_flight_controller: likely,
        usb_vendor_id: vid,
        usb_product_id: pid,
        serial_number,
        manufacturer,
        product,
    }
}

fn is_likely_flight_controller(port: &SerialPortInfo) -> bool {
    if matches!(port.port_type, SerialPortType::UsbPort(_)) {
        return true;
    }
    let name = port.port_name.to_ascii_lowercase();
    name.starts_with("com")
        || name.contains("ttyacm")
        || name.contains("ttyusb")
        || name.contains("cu.usb")
}

fn sanitize_baud_rates(values: Option<Vec<u32>>) -> Result<Vec<u32>, String> {
    let mut values = values.unwrap_or_else(|| DEFAULT_BAUD_RATES.to_vec());
    values.retain(|value| *value > 0);
    values.sort_unstable();
    values.dedup();
    values.reverse();
    if values.is_empty() {
        return Err("At least one non-zero baud rate is required.".to_string());
    }
    Ok(values)
}

fn supported_autopilot(value: MavAutopilot) -> bool {
    matches!(
        value,
        MavAutopilot::MAV_AUTOPILOT_ARDUPILOTMEGA | MavAutopilot::MAV_AUTOPILOT_PX4
    )
}

fn stack_from_autopilot(value: MavAutopilot) -> FlightStack {
    match value {
        MavAutopilot::MAV_AUTOPILOT_ARDUPILOTMEGA => FlightStack::ArduPilot,
        MavAutopilot::MAV_AUTOPILOT_PX4 => FlightStack::Px4,
        MavAutopilot::MAV_AUTOPILOT_GENERIC => FlightStack::Generic,
        _ => FlightStack::Unknown,
    }
}

fn sensor_is_healthy(present: u32, enabled: u32, health: u32, flag: u32) -> bool {
    present & flag != 0 && enabled & flag != 0 && health & flag != 0
}

fn gcs_header() -> MavHeader {
    MavHeader {
        system_id: GCS_SYSTEM_ID,
        component_id: GCS_COMPONENT_ID,
        sequence: 0,
    }
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn percentage_in_text(text: &str) -> Option<u8> {
    text.split(|c: char| !c.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse::<u8>().ok())
        .find(|value| *value <= 100)
}

fn push_event(
    events: &Arc<Mutex<VecDeque<CalibrationProgressEvent>>>,
    event: CalibrationProgressEvent,
) {
    if let Ok(mut events) = events.lock() {
        if events.len() == MAX_PROGRESS_EVENTS {
            events.pop_front();
        }
        events.push_back(event);
    }
}

fn ensure_no_active_connection(state: &DroneState) -> Result<(), String> {
    let guard = state
        .connection
        .lock()
        .map_err(|_| "Drone connection lock is poisoned".to_string())?;
    if guard.is_some() {
        return Err("A drone connection is already active. Disconnect first.".to_string());
    }
    Ok(())
}

fn stop_active_connection(state: &DroneState) -> Result<(), String> {
    let connection = state
        .connection
        .lock()
        .map_err(|_| "Drone connection lock is poisoned".to_string())?
        .take();
    if let Some(connection) = connection {
        connection.stop.store(true, Ordering::Relaxed);
        connection
            .worker
            .join()
            .map_err(|_| "MAVLink reader thread panicked while disconnecting".to_string())?;
    }
    Ok(())
}

fn reset_runtime_data(state: &DroneState) -> Result<(), String> {
    *state
        .telemetry
        .lock()
        .map_err(|_| "Hardware telemetry lock is poisoned".to_string())? =
        HardwareTelemetry::default();
    state
        .calibration_progress
        .lock()
        .map_err(|_| "Calibration progress lock is poisoned".to_string())?
        .clear();
    *state
        .calibration_session
        .lock()
        .map_err(|_| "Calibration session lock is poisoned".to_string())? = None;
    Ok(())
}

fn connection_error(state: &DroneState, port: &str, baud: u32, source: &str) -> String {
    let error = format!("Failed to open {port} at {baud} baud: {source}");
    mark_connection_error(state, error.clone());
    error
}

fn mark_connection_error(state: &DroneState, error: String) {
    let _ = set_status(&state.status, |value| {
        value.connected = false;
        value.connection_phase = ConnectionPhase::Error;
        value.last_error = Some(error);
    });
}

fn get_status_snapshot(status: &Arc<Mutex<DroneStatus>>) -> Result<DroneStatus, String> {
    status
        .lock()
        .map(|value| value.clone())
        .map_err(|_| "Drone status lock is poisoned".to_string())
}

fn set_status<F>(status: &Arc<Mutex<DroneStatus>>, update: F) -> Result<(), String>
where
    F: FnOnce(&mut DroneStatus),
{
    let mut status = status
        .lock()
        .map_err(|_| "Drone status lock is poisoned".to_string())?;
    update(&mut status);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_cross_platform_autopilot_port_names() {
        for name in ["COM3", "/dev/ttyACM0", "/dev/ttyUSB1"] {
            let port = SerialPortInfo {
                port_name: name.to_string(),
                port_type: SerialPortType::Unknown,
            };
            assert!(is_likely_flight_controller(&port), "{name}");
        }
        let system_port = SerialPortInfo {
            port_name: "/dev/ttyS0".to_string(),
            port_type: SerialPortType::Unknown,
        };
        assert!(!is_likely_flight_controller(&system_port));
    }

    #[test]
    fn defaults_and_deduplicates_auto_baud_rates() {
        assert_eq!(sanitize_baud_rates(None).unwrap(), vec![115_200, 57_600]);
        assert_eq!(
            sanitize_baud_rates(Some(vec![57_600, 115_200, 57_600])).unwrap(),
            vec![115_200, 57_600]
        );
        assert!(sanitize_baud_rates(Some(vec![0])).is_err());
    }

    #[test]
    fn extracts_calibration_percentage() {
        assert_eq!(percentage_in_text("calibration progress 73%"), Some(73));
        assert_eq!(percentage_in_text("no progress"), None);
    }
}
