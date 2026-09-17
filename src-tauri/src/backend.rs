use serde::Serialize;
use serde_json::Value;

const ENDPOINT_MANIFEST: &str = include_str!("../endpoints.json");

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendInfo {
    pub application: &'static str,
    pub backend_version: &'static str,
    pub operating_system: &'static str,
    pub architecture: &'static str,
    pub supported_flight_stacks: [&'static str; 2],
    pub supported_transports: [&'static str; 2],
    pub motor_commands_enabled: bool,
}

/// Allows the frontend to read the exact endpoint contract compiled into the app.
#[tauri::command]
pub fn get_backend_manifest() -> Result<Value, String> {
    serde_json::from_str(ENDPOINT_MANIFEST)
        .map_err(|error| format!("The compiled backend endpoint manifest is invalid: {error}"))
}

#[tauri::command]
pub fn get_backend_info() -> BackendInfo {
    BackendInfo {
        application: "Datum",
        backend_version: env!("CARGO_PKG_VERSION"),
        operating_system: std::env::consts::OS,
        architecture: std::env::consts::ARCH,
        supported_flight_stacks: ["ArduPilot", "PX4"],
        supported_transports: ["Tauri invoke", "MAVLink serial"],
        motor_commands_enabled: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_manifest_is_valid_json() {
        let manifest = get_backend_manifest().expect("endpoint manifest must parse");
        assert!(manifest["endpoints"].is_array());
    }
}
