mod calibration;
mod endpoint_manifest;
mod logging;
mod server;
use tauri::Manager;

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(calibration::CalibrationService::default())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            greet,
            calibration::vehicle_status,
            calibration::list_serial_ports,
            calibration::calibration_options,
            calibration::frame_options,
            calibration::current_frame,
            calibration::set_frame,
            calibration::connect_vehicle,
            calibration::connect_serial_vehicle,
            calibration::disconnect_vehicle,
            calibration::start_calibration,
            calibration::start_accelerometer_calibration,
            calibration::accelerometer_calibration_step,
            calibration::accelerometer_calibration_status,
            calibration::cancel_accelerometer_calibration,
            calibration::compass_sensors,
            calibration::start_compass_calibration,
            calibration::compass_calibration_status,
            calibration::accept_compass_calibration,
            calibration::cancel_compass_calibration,
            calibration::retry_compass_calibration,
            calibration::radio_channels,
            calibration::start_radio_calibration,
            calibration::radio_calibration_status,
            calibration::finish_radio_calibration,
            calibration::cancel_radio_calibration,
            calibration::esc_capabilities,
            calibration::esc_protocol_options,
            calibration::esc_steps,
            calibration::start_esc_calibration,
            calibration::advance_esc_calibration,
            calibration::esc_calibration_status,
            calibration::cancel_esc_calibration,
            calibration::flight_mode_configuration,
            calibration::flight_mode_options,
            calibration::set_flight_mode_configuration,
            calibration::failsafe_configuration,
            calibration::failsafe_options,
            calibration::set_failsafe_configuration,
            calibration::battery_configuration,
            calibration::battery_monitor_options,
            calibration::set_battery_configuration,
            calibration::calibrate_battery_voltage,
            calibration::run_motor_test,
            calibration::radio_calibration_options,
            calibration::continue_calibration,
            calibration::stop_calibration
        ])
        .setup(move |app| {
            let log_dir = app
                .path()
                .app_log_dir()
                .map_err(|error| format!("Could not resolve application log directory: {error}"))?;
            let log_path = logging::init(&log_dir)?;
            log::info!(target: "datum::startup", "backend starting log_path={}", log_path.display());
            server::start()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
