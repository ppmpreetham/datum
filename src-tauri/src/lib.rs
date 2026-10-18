mod calibration;
mod server;

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
            calibration::calibration_options,
            calibration::connect_vehicle,
            calibration::disconnect_vehicle,
            calibration::start_calibration,
            calibration::continue_calibration,
            calibration::stop_calibration
        ])
        .setup(move |_app| {
            server::start();
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
