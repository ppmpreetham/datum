use crate::drone::{self, FlightStack, MavlinkCommandReceipt};
use ::mavlink::dialects::ardupilotmega::MavCmd;
use serde::Serialize;
use std::collections::BTreeMap;

const CONFIG: &str = include_str!("../calibration_config.yaml");
const SENSOR_IDS: [&str; 11] = [
    "accelerometer",
    "gyroscope",
    "compass",
    "level_horizon",
    "radio_controller",
    "battery_voltage",
    "battery_current",
    "esc_throttle_range",
    "airspeed",
    "optical_flow",
    "imu_temperature_compensation",
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationCapabilities {
    pub api_enabled: BTreeMap<String, bool>,
    pub supported_flight_stacks: [&'static str; 2],
    pub motors_used: bool,
    pub propellers_must_be_removed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationStart {
    pub calibration: &'static str,
    pub flight_stack: FlightStack,
    pub command_sent: MavlinkCommandReceipt,
    pub next_step: &'static str,
}

#[tauri::command]
pub fn get_calibration_capabilities() -> Result<CalibrationCapabilities, String> {
    Ok(CalibrationCapabilities {
        api_enabled: parse_api_enabled(CONFIG)?,
        supported_flight_stacks: ["ArduPilot", "PX4"],
        motors_used: false,
        propellers_must_be_removed: true,
    })
}

#[tauri::command]
pub fn start_accelerometer_calibration(
    state: tauri::State<'_, drone::DroneState>,
) -> Result<CalibrationStart, String> {
    let stack = drone::flight_stack(&state)?;
    let next_step = match stack {
        FlightStack::ArduPilot => {
            "Wait for an orientation prompt, place the vehicle as shown, then call confirm_accelerometer_orientation. Keep propellers removed."
        }
        FlightStack::Px4 => {
            "Follow each PX4 orientation prompt. PX4 detects when the vehicle is held still; no confirmation call is required. Keep propellers removed."
        }
        _ => return Err("Accelerometer calibration requires ArduPilot or PX4.".to_string()),
    };
    start_calibration(
        &state,
        "accelerometer",
        MavCmd::MAV_CMD_PREFLIGHT_CALIBRATION,
        [0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        next_step,
    )
}

/// ArduPilot's six-position accelerometer protocol requires the GCS to confirm
/// the requested position. PX4 detects stable orientations without this call.
#[tauri::command]
pub fn confirm_accelerometer_orientation(
    state: tauri::State<'_, drone::DroneState>,
    position: String,
) -> Result<MavlinkCommandReceipt, String> {
    if drone::flight_stack(&state)? != FlightStack::ArduPilot {
        return Err("PX4 detects accelerometer orientations automatically; this confirmation endpoint is only for ArduPilot.".to_string());
    }
    drone::require_calibration_session(&state, "accelerometer", true)?;
    let (value, instruction) = match position.as_str() {
        "level" => (
            1.0,
            "Confirmed level position; hold still and wait for the next prompt.",
        ),
        "left" => (
            2.0,
            "Confirmed left-side position; hold still and wait for the next prompt.",
        ),
        "right" => (
            3.0,
            "Confirmed right-side position; hold still and wait for the next prompt.",
        ),
        "nose_down" => (
            4.0,
            "Confirmed nose-down position; hold still and wait for the next prompt.",
        ),
        "nose_up" => (
            5.0,
            "Confirmed nose-up position; hold still and wait for the next prompt.",
        ),
        "back" => (
            6.0,
            "Confirmed upside-down position; hold still and wait for the result.",
        ),
        _ => {
            return Err(
                "Unknown orientation. Use: level, left, right, nose_down, nose_up, or back."
                    .to_string(),
            )
        }
    };
    let receipt = drone::send_command_long(
        &state,
        MavCmd::MAV_CMD_ACCELCAL_VEHICLE_POS,
        [value, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    )?;
    drone::update_calibration_instruction(&state, "accelerometer", instruction)?;
    Ok(receipt)
}

#[tauri::command]
pub fn start_gyroscope_calibration(
    state: tauri::State<'_, drone::DroneState>,
) -> Result<CalibrationStart, String> {
    start_calibration(
        &state,
        "gyroscope",
        MavCmd::MAV_CMD_PREFLIGHT_CALIBRATION,
        [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        "Keep the vehicle completely still until calibration completes. Keep propellers removed.",
    )
}

#[tauri::command]
pub fn start_compass_calibration(
    state: tauri::State<'_, drone::DroneState>,
) -> Result<CalibrationStart, String> {
    match drone::flight_stack(&state)? {
        FlightStack::ArduPilot => start_calibration(
            &state,
            "compass",
            MavCmd::MAV_CMD_DO_START_MAG_CAL,
            // All enabled compasses, retry failures, autosave, no reboot.
            [0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0],
            "Rotate the vehicle slowly through every orientation, away from metal and magnets. Progress and the final MAG_CAL_REPORT are tracked by the backend.",
        ),
        FlightStack::Px4 => start_calibration(
            &state,
            "compass",
            MavCmd::MAV_CMD_PREFLIGHT_CALIBRATION,
            [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            "Follow the PX4 compass prompts and rotate the vehicle through every orientation, away from metal and magnets.",
        ),
        _ => Err("Compass calibration requires ArduPilot or PX4.".to_string()),
    }
}

#[tauri::command]
pub fn cancel_compass_calibration(
    state: tauri::State<'_, drone::DroneState>,
) -> Result<MavlinkCommandReceipt, String> {
    if drone::flight_stack(&state)? != FlightStack::ArduPilot {
        return Err(
            "Structured MAVLink compass cancellation is currently available only for ArduPilot."
                .to_string(),
        );
    }
    drone::require_calibration_session(&state, "compass", true)?;
    drone::send_command_long(&state, MavCmd::MAV_CMD_DO_CANCEL_MAG_CAL, [0.0; 7])
}

#[tauri::command]
pub fn accept_compass_calibration(
    state: tauri::State<'_, drone::DroneState>,
) -> Result<MavlinkCommandReceipt, String> {
    if drone::flight_stack(&state)? != FlightStack::ArduPilot {
        return Err(
            "Structured MAVLink compass acceptance is currently available only for ArduPilot."
                .to_string(),
        );
    }
    drone::require_calibration_session(&state, "compass", false)?;
    drone::send_command_long(&state, MavCmd::MAV_CMD_DO_ACCEPT_MAG_CAL, [0.0; 7])
}

#[tauri::command]
pub fn start_radio_controller_calibration(
    state: tauri::State<'_, drone::DroneState>,
) -> Result<CalibrationStart, String> {
    start_calibration(
        &state,
        "radio_controller",
        MavCmd::MAV_CMD_PREFLIGHT_CALIBRATION,
        [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0],
        "Power the transmitter, then move every stick and configured switch through its full range. Firmware may report UNSUPPORTED when it requires parameter-based RC calibration.",
    )
}

#[tauri::command]
pub fn set_level_horizon(
    state: tauri::State<'_, drone::DroneState>,
) -> Result<CalibrationStart, String> {
    start_calibration(
        &state,
        "level_horizon",
        MavCmd::MAV_CMD_PREFLIGHT_CALIBRATION,
        [0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0],
        "Leave the vehicle motionless on a verified level surface until the flight controller reports completion. Keep propellers removed.",
    )
}

fn start_calibration(
    state: &drone::DroneState,
    sensor_id: &'static str,
    command: MavCmd,
    params: [f32; 7],
    next_step: &'static str,
) -> Result<CalibrationStart, String> {
    if !is_calibration_api_enabled(sensor_id)? {
        return Err(format!(
            "{sensor_id} calibration API is disabled in calibration_config.yaml."
        ));
    }
    let stack = drone::flight_stack(state)?;
    drone::begin_calibration_session(state, sensor_id, command, next_step)?;
    let command_sent = match drone::send_command_long(state, command, params) {
        Ok(receipt) => receipt,
        Err(error) => {
            drone::fail_calibration_start(state, &error);
            return Err(error);
        }
    };
    Ok(CalibrationStart {
        calibration: sensor_id,
        flight_stack: stack,
        command_sent,
        next_step,
    })
}

pub fn is_calibration_api_enabled(sensor_id: &str) -> Result<bool, String> {
    let flags = parse_api_enabled(CONFIG)?;
    flags
        .get(sensor_id)
        .copied()
        .ok_or_else(|| format!("Unknown calibration sensor id: {sensor_id}"))
}

fn parse_api_enabled(config: &str) -> Result<BTreeMap<String, bool>, String> {
    let mut in_api_enabled = false;
    let mut flags = BTreeMap::new();
    for raw_line in config.lines() {
        let line = raw_line
            .split_once('#')
            .map_or(raw_line, |(before, _)| before);
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == "api_enabled:" {
            in_api_enabled = true;
            continue;
        }
        if !in_api_enabled || !(line.starts_with(' ') || line.starts_with('\t')) {
            continue;
        }
        let (sensor_id, enabled) = trimmed
            .split_once(':')
            .ok_or_else(|| format!("Invalid calibration configuration line: {trimmed}"))?;
        let sensor_id = sensor_id.trim();
        let enabled = match enabled.trim() {
            "true" => true,
            "false" => false,
            value => {
                return Err(format!(
                    "Calibration flag for {sensor_id} must be true or false, found: {value}"
                ))
            }
        };
        if !SENSOR_IDS.contains(&sensor_id) {
            return Err(format!(
                "Unknown calibration sensor id in YAML: {sensor_id}"
            ));
        }
        if flags.insert(sensor_id.to_string(), enabled).is_some() {
            return Err(format!(
                "Duplicate calibration sensor id in YAML: {sensor_id}"
            ));
        }
    }
    for sensor_id in SENSOR_IDS {
        if !flags.contains_key(sensor_id) {
            return Err(format!(
                "Missing calibration flag in YAML for sensor: {sensor_id}"
            ));
        }
    }
    Ok(flags)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_calibration_flags() {
        assert_eq!(is_calibration_api_enabled("accelerometer"), Ok(true));
        assert_eq!(is_calibration_api_enabled("battery_current"), Ok(false));
    }

    #[test]
    fn rejects_missing_calibration_flag() {
        let error = parse_api_enabled("api_enabled:\n  accelerometer: true\n")
            .expect_err("configuration should require every sensor flag");
        assert!(error.contains("Missing calibration flag"));
    }
}
