#![allow(dead_code)]

pub const REGISTERED_COMMANDS: &[&str] = &[
    "greet",
    "vehicle_status",
    "list_serial_ports",
    "calibration_options",
    "frame_options",
    "current_frame",
    "set_frame",
    "connect_vehicle",
    "connect_serial_vehicle",
    "disconnect_vehicle",
    "start_calibration",
    "start_accelerometer_calibration",
    "accelerometer_calibration_step",
    "accelerometer_calibration_status",
    "cancel_accelerometer_calibration",
    "compass_sensors",
    "start_compass_calibration",
    "compass_calibration_status",
    "accept_compass_calibration",
    "cancel_compass_calibration",
    "retry_compass_calibration",
    "radio_channels",
    "start_radio_calibration",
    "radio_calibration_status",
    "finish_radio_calibration",
    "cancel_radio_calibration",
    "esc_capabilities",
    "esc_steps",
    "start_esc_calibration",
    "advance_esc_calibration",
    "esc_calibration_status",
    "cancel_esc_calibration",
    "flight_mode_configuration",
    "set_flight_mode_configuration",
    "failsafe_configuration",
    "set_failsafe_configuration",
    "battery_configuration",
    "set_battery_configuration",
    "calibrate_battery_voltage",
    "run_motor_test",
    "continue_calibration",
    "stop_calibration",
];

pub fn registered_command_names() -> &'static [&'static str] {
    REGISTERED_COMMANDS
}

pub fn manifest_command_names(json: &str) -> Result<Vec<String>, String> {
    let manifest: serde_json::Value = serde_json::from_str(json)
        .map_err(|error| format!("Invalid endpoint manifest: {error}"))?;
    manifest["endpoints"]
        .as_array()
        .ok_or("Endpoint manifest must contain an endpoints array.".to_string())?
        .iter()
        .map(|endpoint| {
            endpoint["id"]
                .as_str()
                .map(str::to_string)
                .ok_or("Every endpoint must contain a string id.".to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn manifest_matches_registered_tauri_commands_without_duplicates() {
        let manifest_names = manifest_command_names(include_str!("../endpoints.json")).unwrap();
        let unique_manifest = manifest_names.iter().cloned().collect::<BTreeSet<_>>();
        let registered = registered_command_names()
            .iter()
            .map(|name| (*name).to_string())
            .collect::<BTreeSet<_>>();

        assert_eq!(manifest_names.len(), unique_manifest.len());
        assert_eq!(unique_manifest, registered);
    }
}
