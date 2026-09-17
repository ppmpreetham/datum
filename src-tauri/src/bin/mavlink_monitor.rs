use datum_lib::mavlink::{
    command_long_v1, Attitude, Heartbeat, MavlinkMessage, MavlinkParser, SystemStatus,
    MAV_CMD_PREFLIGHT_CALIBRATION,
};
use datum_lib::server;
use std::{
    collections::HashSet,
    env,
    io::{Read, Write},
    process,
    time::Duration,
};

const READ_TIMEOUT: Duration = Duration::from_millis(200);
const MAV_CMD_SET_MESSAGE_INTERVAL: u16 = 511;
const SYS_STATUS_MESSAGE_ID: u32 = 1;
const ATTITUDE_MESSAGE_ID: u32 = 30;

fn main() {
    if let Err(error) = run() {
        eprintln!("mavlink-monitor: {error}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut arguments = env::args().skip(1);
    let port_name = arguments.next().ok_or_else(usage)?;
    let baud_rate = arguments
        .next()
        .map(|value| value.parse::<u32>().map_err(|_| usage()))
        .transpose()?
        .unwrap_or(115_200);
    let calibration = arguments
        .next()
        .map(|value| calibration_request(&value))
        .transpose()?;
    let gyro_requested = calibration.is_some_and(|(name, _)| name == "gyro");

    if arguments.next().is_some() {
        return Err(usage());
    }

    let mut port = serialport::new(&port_name, baud_rate)
        .timeout(READ_TIMEOUT)
        .open()
        .map_err(|error| format!("could not open {port_name}: {error}"))?;
    server::start();

    println!("Connected to {port_name} at {baud_rate} baud.");
    println!("WebSocket attitude stream available at ws://127.0.0.1:8000/ws/gyro.");
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
    let flight_stack = match heartbeat.autopilot {
        3 => "ArduPilot",
        12 => "PX4",
        _ => "unknown autopilot",
    };
    println!(
        "HEARTBEAT system={} component={} stack={flight_stack}",
        heartbeat.system_id, heartbeat.component_id
    );
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
    "usage: cargo run --bin mavlink_monitor -- /dev/ttyACM0 [baud] [accelerometer|gyro|compass|radio|level]".to_string()
}
