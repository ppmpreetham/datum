#[path = "../mavlink.rs"]
mod mavlink;
#[path = "../server.rs"]
mod server;

use mavlink::{
    command_long_v1, Attitude, Heartbeat, MavlinkMessage, MavlinkParser, SystemStatus,
    MAV_CMD_PREFLIGHT_CALIBRATION,
};
use std::{
    collections::HashSet,
    env,
    io::{Read, Write},
    process,
    time::{Duration, Instant},
};

const READ_TIMEOUT: Duration = Duration::from_millis(200);
const AUTO_SCAN_TIMEOUT: Duration = Duration::from_millis(1800);
const AUTO_RETRY_DELAY: Duration = Duration::from_secs(2);
const DEFAULT_BAUD_RATE: u32 = 115_200;
const DEFAULT_AUTO_BAUD_RATES: [u32; 5] = [115_200, 57_600, 921_600, 460_800, 230_400];
const MAV_CMD_SET_MESSAGE_INTERVAL: u16 = 511;
const SYS_STATUS_MESSAGE_ID: u32 = 1;
const ATTITUDE_MESSAGE_ID: u32 = 30;

#[derive(Debug)]
enum PortSelection {
    Manual(String),
    Auto,
}

#[derive(Debug)]
struct MonitorArgs {
    port_selection: PortSelection,
    baud_rates: Vec<u32>,
    calibration: Option<(&'static str, [f32; 7])>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("mavlink-monitor: {error}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = parse_args(env::args().skip(1))?;
    let calibration = args.calibration;
    let gyro_requested = calibration.is_some_and(|(name, _)| name == "gyro");

    start_phone_gyro_fallback_server();

    let (port_name, baud_rate, mut port) =
        open_or_wait_selected_port(args.port_selection, &args.baud_rates)?;

    println!("Connected to {port_name} at {baud_rate} baud.");
    if let Some((name, _)) = calibration {
        println!("Waiting for an ArduPilot or PX4 heartbeat before starting {name} calibration.");
    } else {
        println!(
            "Monitoring MAVLink status text and command acknowledgements. Press Ctrl+C to stop."
        );
    }

    let mut parser = MavlinkParser::new();
    let mut buffer = [0_u8; 512];
    let mut calibration_sent = false;
    let mut health_subscription_sent = false;
    let mut sequence = 0_u8;
    let mut last_heartbeat = None;
    let mut seen_status_text = HashSet::new();
    let mut last_sensor_health = None;
    let mut gyro_was_unhealthy_after_start = false;
    let mut gyro_command_accepted = false;
    let mut gyro_completion_reported = false;

    loop {
        match port.read(&mut buffer) {
            Ok(bytes_read) if bytes_read > 0 => {
                for message in parser.push(&buffer[..bytes_read]) {
                    match message {
                        MavlinkMessage::Heartbeat(heartbeat) => {
                            let heartbeat_id = (
                                heartbeat.system_id,
                                heartbeat.component_id,
                                heartbeat.autopilot,
                            );
                            if last_heartbeat != Some(heartbeat_id) {
                                print_heartbeat(&heartbeat);
                                last_heartbeat = Some(heartbeat_id);
                            }

                            if !health_subscription_sent
                                && is_supported_autopilot(heartbeat.autopilot)
                            {
                                for (message_id, interval_us) in [
                                    (SYS_STATUS_MESSAGE_ID, 1_000_000_u32),
                                    (ATTITUDE_MESSAGE_ID, 50_000_u32),
                                ] {
                                    let frame = command_long_v1(
                                        sequence,
                                        heartbeat.system_id,
                                        heartbeat.component_id,
                                        MAV_CMD_SET_MESSAGE_INTERVAL,
                                        0,
                                        [
                                            message_id as f32,
                                            interval_us as f32,
                                            0.0,
                                            0.0,
                                            0.0,
                                            0.0,
                                            0.0,
                                        ],
                                    );
                                    port.write_all(&frame).map_err(|error| {
                                        format!(
                                            "could not request MAVLink message {message_id}: {error}"
                                        )
                                    })?;
                                    port.flush().map_err(|error| {
                                        format!(
                                            "could not flush MAVLink message {message_id} request: {error}"
                                        )
                                    })?;
                                    sequence = sequence.wrapping_add(1);
                                }
                                println!("Requested SYS_STATUS at 1 Hz and ATTITUDE at 20 Hz.");
                                health_subscription_sent = true;
                            }

                            if !calibration_sent {
                                if let Some((name, params)) = calibration {
                                    if !is_supported_autopilot(heartbeat.autopilot) {
                                        continue;
                                    }

                                    let frame = command_long_v1(
                                        sequence,
                                        heartbeat.system_id,
                                        heartbeat.component_id,
                                        MAV_CMD_PREFLIGHT_CALIBRATION,
                                        0,
                                        params,
                                    );
                                    port.write_all(&frame).map_err(|error| {
                                        format!(
                                            "could not send {name} calibration command: {error}"
                                        )
                                    })?;
                                    port.flush().map_err(|error| {
                                        format!(
                                            "could not flush {name} calibration command: {error}"
                                        )
                                    })?;
                                    println!(
                                        "Sent {name} calibration command to system {}, component {}. Waiting for MAVLink COMMAND_ACK and STATUSTEXT messages.",
                                        heartbeat.system_id, heartbeat.component_id
                                    );
                                    calibration_sent = true;
                                    sequence = sequence.wrapping_add(1);
                                }
                            }
                        }
                        MavlinkMessage::CommandAck(ack) => {
                            println!(
                                "COMMAND_ACK command={} result={}{}",
                                ack.command,
                                mav_result_name(ack.result),
                                ack.progress
                                    .map(|progress| format!(" progress={progress}%"))
                                    .unwrap_or_default()
                            );
                            if gyro_requested
                                && ack.command == MAV_CMD_PREFLIGHT_CALIBRATION
                                && ack.result == 0
                            {
                                gyro_command_accepted = true;
                            }
                        }
                        MavlinkMessage::StatusText(status) => {
                            let status_id = (status.severity, status.text.clone());
                            if seen_status_text.insert(status_id) {
                                println!(
                                    "STATUSTEXT severity={} text={}",
                                    status.severity, status.text
                                );
                            }
                        }
                        MavlinkMessage::SystemStatus(system_status) => {
                            let health_id = (
                                system_status.sensors_present,
                                system_status.sensors_enabled,
                                system_status.sensors_health,
                            );
                            if last_sensor_health != Some(health_id) {
                                print_sensor_health(&system_status);
                                last_sensor_health = Some(health_id);
                            }

                            if gyro_requested && calibration_sent {
                                let gyro_healthy = sensor_is_healthy(&system_status, 1);
                                if !gyro_healthy {
                                    gyro_was_unhealthy_after_start = true;
                                }
                                if gyro_command_accepted
                                    && gyro_was_unhealthy_after_start
                                    && gyro_healthy
                                    && !gyro_completion_reported
                                {
                                    println!(
                                        "CALIBRATION_RESULT gyro=likely_complete: command accepted and gyro health was restored."
                                    );
                                    gyro_completion_reported = true;
                                }
                            }
                        }
                        MavlinkMessage::Attitude(attitude) => {
                            publish_attitude_to_websocket(attitude);
                        }
                        _ => {}
                    }
                }
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {}
            Err(error) => return Err(format!("serial read failed: {error}")),
        }
    }
}

fn start_phone_gyro_fallback_server() {
    let _ = server::start();
    println!("WebSocket attitude stream available at ws://127.0.0.1:8000/ws/gyro.");
    println!("Phone gyro fallback is active while the drone is disconnected.");
    println!("Open the app's /sender page on a phone and tap Start Sharing Gyroscope.");
}

fn parse_args(arguments: impl IntoIterator<Item = String>) -> Result<MonitorArgs, String> {
    let mut arguments = arguments.into_iter();
    let first = arguments.next().ok_or_else(usage)?;
    let port_selection = if matches!(first.as_str(), "auto" | "--auto") {
        PortSelection::Auto
    } else {
        PortSelection::Manual(first)
    };

    let mut baud_rates = match port_selection {
        PortSelection::Auto => DEFAULT_AUTO_BAUD_RATES.to_vec(),
        PortSelection::Manual(_) => vec![DEFAULT_BAUD_RATE],
    };
    let mut baud_was_set = false;
    let mut calibration = None;

    for argument in arguments {
        if matches!(argument.as_str(), "status" | "monitor") {
            continue;
        }

        if let Some(parsed_baud_rates) = parse_baud_rates(&argument) {
            if baud_was_set {
                return Err(usage());
            }
            baud_rates = parsed_baud_rates;
            baud_was_set = true;
            continue;
        }

        if calibration.is_some() {
            return Err(usage());
        }
        calibration = Some(calibration_request(&argument)?);
    }

    Ok(MonitorArgs {
        port_selection,
        baud_rates,
        calibration,
    })
}

fn parse_baud_rates(value: &str) -> Option<Vec<u32>> {
    let mut baud_rates = Vec::new();
    for part in value.split(',') {
        let baud_rate = part.parse::<u32>().ok()?;
        if baud_rate == 0 {
            return None;
        }
        if !baud_rates.contains(&baud_rate) {
            baud_rates.push(baud_rate);
        }
    }
    Some(baud_rates)
}

fn open_or_wait_selected_port(
    port_selection: PortSelection,
    baud_rates: &[u32],
) -> Result<(String, u32, Box<dyn serialport::SerialPort>), String> {
    match port_selection {
        PortSelection::Manual(port_name) => {
            let baud_rate = baud_rates.first().copied().unwrap_or(DEFAULT_BAUD_RATE);
            let port = open_serial_port(&port_name, baud_rate)?;
            Ok((port_name, baud_rate, port))
        }
        PortSelection::Auto => wait_for_auto_port(baud_rates),
    }
}

fn wait_for_auto_port(
    baud_rates: &[u32],
) -> Result<(String, u32, Box<dyn serialport::SerialPort>), String> {
    println!("Waiting for an ArduPilot or PX4 serial port. Press Ctrl+C to stop.");

    let mut scan_count = 0_u64;
    loop {
        scan_count += 1;
        match auto_open_port(baud_rates) {
            Ok(connection) => return Ok(connection),
            Err(error) => {
                println!("DRONE_CONNECTION connected=false scan={scan_count} reason=\"{error}\"");
                println!(
                    "No flight controller found yet. Plug in the drone and keep this running."
                );
                std::thread::sleep(AUTO_RETRY_DELAY);
            }
        }
    }
}

fn open_serial_port(
    port_name: &str,
    baud_rate: u32,
) -> Result<Box<dyn serialport::SerialPort>, String> {
    serialport::new(port_name, baud_rate)
        .timeout(READ_TIMEOUT)
        .open()
        .map_err(|error| format!("could not open {port_name} at {baud_rate} baud: {error}"))
}

fn auto_open_port(
    baud_rates: &[u32],
) -> Result<(String, u32, Box<dyn serialport::SerialPort>), String> {
    let port_names = auto_port_candidates()?;

    if port_names.is_empty() {
        return Err("no serial ports were detected".to_string());
    }

    println!(
        "Auto scanning {} serial port candidate(s) at baud rate(s): {}",
        port_names.len(),
        baud_rates
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    );

    let mut attempts = Vec::new();
    let print_open_failures = port_names.len() <= 32;
    for port_name in port_names {
        for baud_rate in baud_rates {
            let mut port = match open_serial_port(&port_name, *baud_rate) {
                Ok(port) => port,
                Err(error) => {
                    if print_open_failures {
                        println!("Checking {port_name} at {baud_rate} baud... open failed");
                    }
                    attempts.push(format!("{port_name}@{baud_rate}: {error}"));
                    continue;
                }
            };

            print!("Checking {port_name} at {baud_rate} baud... ");
            match wait_for_supported_heartbeat(&mut *port) {
                Ok(Some(heartbeat)) => {
                    println!("found {}", flight_stack_name(heartbeat.autopilot));
                    print_heartbeat(&heartbeat);
                    return Ok((port_name, *baud_rate, port));
                }
                Ok(None) => {
                    println!("no MAVLink heartbeat");
                    attempts.push(format!("{port_name}@{baud_rate}: no supported heartbeat"));
                }
                Err(error) => {
                    println!("read failed");
                    attempts.push(format!("{port_name}@{baud_rate}: {error}"));
                }
            }
        }
    }

    Err(format!(
        "no ArduPilot or PX4 heartbeat found. Tried: {}",
        attempt_summary(&attempts)
    ))
}

fn auto_port_candidates() -> Result<Vec<String>, String> {
    let mut port_names = serialport::available_ports()
        .map_err(|error| format!("could not list serial ports: {error}"))?
        .into_iter()
        .map(|port| port.port_name)
        .collect::<Vec<_>>();

    port_names.extend(platform_auto_port_fallbacks());
    port_names.sort();
    port_names.dedup();
    Ok(port_names)
}

#[cfg(windows)]
fn platform_auto_port_fallbacks() -> Vec<String> {
    (1..=256).map(|number| format!("COM{number}")).collect()
}

#[cfg(not(windows))]
fn platform_auto_port_fallbacks() -> Vec<String> {
    Vec::new()
}

fn attempt_summary(attempts: &[String]) -> String {
    const MAX_ATTEMPTS_TO_PRINT: usize = 20;

    if attempts.len() <= MAX_ATTEMPTS_TO_PRINT {
        return attempts.join("; ");
    }

    format!(
        "{} attempt(s); first {}: {}",
        attempts.len(),
        MAX_ATTEMPTS_TO_PRINT,
        attempts[..MAX_ATTEMPTS_TO_PRINT].join("; ")
    )
}

fn wait_for_supported_heartbeat(
    port: &mut dyn serialport::SerialPort,
) -> Result<Option<Heartbeat>, String> {
    let started_at = Instant::now();
    let mut parser = MavlinkParser::new();
    let mut buffer = [0_u8; 512];

    while started_at.elapsed() < AUTO_SCAN_TIMEOUT {
        match port.read(&mut buffer) {
            Ok(bytes_read) if bytes_read > 0 => {
                for message in parser.push(&buffer[..bytes_read]) {
                    if let MavlinkMessage::Heartbeat(heartbeat) = message {
                        if is_supported_autopilot(heartbeat.autopilot) {
                            return Ok(Some(heartbeat));
                        }
                    }
                }
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {}
            Err(error) => return Err(format!("serial read failed: {error}")),
        }
    }

    Ok(None)
}

fn calibration_request(name: &str) -> Result<(&'static str, [f32; 7]), String> {
    match name {
        "accelerometer" => Ok(("accelerometer", [0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0])),
        "gyro" => Ok(("gyro", [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0])),
        "compass" => Ok(("compass", [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0])),
        "radio" => Ok(("radio", [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0])),
        "level" => Ok(("level horizon", [0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0])),
        _ => Err(usage()),
    }
}

fn is_supported_autopilot(autopilot: u8) -> bool {
    matches!(autopilot, 3 | 12)
}

fn print_heartbeat(heartbeat: &Heartbeat) {
    let flight_stack = flight_stack_name(heartbeat.autopilot);
    println!(
        "HEARTBEAT system={} component={} stack={flight_stack}",
        heartbeat.system_id, heartbeat.component_id
    );
}

fn flight_stack_name(autopilot: u8) -> &'static str {
    match autopilot {
        3 => "ArduPilot",
        12 => "PX4",
        _ => "unknown autopilot",
    }
}

fn mav_result_name(result: u8) -> &'static str {
    match result {
        0 => "ACCEPTED",
        1 => "TEMPORARILY_REJECTED",
        2 => "DENIED",
        3 => "UNSUPPORTED",
        4 => "FAILED",
        5 => "IN_PROGRESS",
        6 => "CANCELLED",
        _ => "UNKNOWN",
    }
}

fn print_sensor_health(system_status: &SystemStatus) {
    const GYRO: u32 = 1;
    const ACCELEROMETER: u32 = 1 << 1;
    const COMPASS: u32 = 1 << 2;
    const BAROMETER: u32 = 1 << 3;
    const GPS: u32 = 1 << 5;

    println!(
        "SENSOR_HEALTH gyro={} accel={} compass={} barometer={} gps={}",
        sensor_is_healthy(system_status, GYRO),
        sensor_is_healthy(system_status, ACCELEROMETER),
        sensor_is_healthy(system_status, COMPASS),
        sensor_is_healthy(system_status, BAROMETER),
        sensor_is_healthy(system_status, GPS),
    );
}

fn sensor_is_healthy(system_status: &SystemStatus, flag: u32) -> bool {
    system_status.sensors_present & flag != 0
        && system_status.sensors_enabled & flag != 0
        && system_status.sensors_health & flag != 0
}

fn publish_attitude_to_websocket(attitude: Attitude) {
    server::publish_drone_attitude(attitude.roll_rad, attitude.pitch_rad, attitude.yaw_rad);
}

fn usage() -> String {
    "usage: cargo run --bin mavlink_monitor -- auto [baud|baud,baud] [status|accelerometer|gyro|compass|radio|level]\n       cargo run --bin mavlink_monitor -- <port> [baud] [status|accelerometer|gyro|compass|radio|level]".to_string()
}
