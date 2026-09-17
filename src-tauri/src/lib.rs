mod backend;
mod calibration;
mod drone;
pub mod mavlink;
pub mod server;

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(drone::DroneState::default())
        .invoke_handler(tauri::generate_handler![
            greet,
            backend::get_backend_info,
            backend::get_backend_manifest,
            calibration::get_calibration_capabilities,
            calibration::start_accelerometer_calibration,
            calibration::confirm_accelerometer_orientation,
            calibration::start_compass_calibration,
            calibration::accept_compass_calibration,
            calibration::cancel_compass_calibration,
            calibration::start_gyroscope_calibration,
            calibration::start_radio_controller_calibration,
            calibration::set_level_horizon,
            drone::list_serial_ports,
            drone::connect_drone,
            drone::auto_connect_drone,
            drone::disconnect_drone,
            drone::get_drone_status,
            drone::get_hardware_telemetry,
            drone::get_calibration_progress,
            drone::get_current_calibration,
            drone::request_hardware_telemetry,
            server::get_stream_server_status,
        ])
        .setup(move |_app| {
            server::start();
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
