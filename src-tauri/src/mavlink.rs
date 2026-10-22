#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heartbeat {
    pub system_id: u8,
    pub component_id: u8,
    pub autopilot: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemStatus {
    pub sensors_present: u32,
    pub sensors_enabled: u32,
    pub sensors_health: u32,
    pub voltage_battery_mv: u16,
    pub current_battery_ca: i16,
    pub battery_remaining_percent: i8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScaledImu {
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpsRawInt {
    pub fix_type: u8,
    pub satellites_visible: u8,
    pub latitude_e7: i32,
    pub longitude_e7: i32,
    pub altitude_mm: i32,
    pub horizontal_accuracy_cm: u16,
    pub vertical_accuracy_cm: u16,
    pub velocity_cm_s: u16,
    pub course_over_ground_cdeg: u16,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Attitude {
    pub time_boot_ms: u32,
    pub roll_rad: f32,
    pub pitch_rad: f32,
    pub yaw_rad: f32,
    pub rollspeed_rad_s: f32,
    pub pitchspeed_rad_s: f32,
    pub yawspeed_rad_s: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandAck {
    pub command: u16,
    pub result: u8,
    pub progress: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusText {
    pub severity: u8,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MavlinkMessage {
    Heartbeat(Heartbeat),
    SystemStatus(SystemStatus),
    ScaledImu(ScaledImu),
    GpsRawInt(GpsRawInt),
    Attitude(Attitude),
    CommandAck(CommandAck),
    StatusText(StatusText),
}

#[derive(Default)]
pub struct MavlinkParser {
    buffer: Vec<u8>,
}

impl MavlinkParser {
    pub fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    pub fn push(&mut self, data: &[u8]) -> Vec<MavlinkMessage> {
        self.buffer.extend_from_slice(data);
        let mut messages = Vec::new();

        loop {
            let Some(start) = self
                .buffer
                .iter()
                .position(|byte| *byte == MAVLINK_V1_STX || *byte == MAVLINK_V2_STX)
            else {
                self.buffer.clear();
                break;
            };

            if start > 0 {
                self.buffer.drain(..start);
            }

            let Some(frame_len) = expected_frame_len(&self.buffer) else {
                break;
            };

            if self.buffer.len() < frame_len {
                break;
            }

            let frame: Vec<u8> = self.buffer.drain(..frame_len).collect();
            if let Some(message) = parse_mavlink_frame(&frame) {
                messages.push(message);
            }
        }

        messages
    }
}

const MAVLINK_V1_STX: u8 = 0xfe;
const MAVLINK_V2_STX: u8 = 0xfd;
const MAVLINK_V2_SIGNED_FLAG: u8 = 0x01;
const HEARTBEAT_MESSAGE_ID: u32 = 0;
const HEARTBEAT_CRC_EXTRA: u8 = 50;
const HEARTBEAT_MIN_PAYLOAD_LEN: usize = 6;
const SYSTEM_STATUS_MESSAGE_ID: u32 = 1;
const SYSTEM_STATUS_CRC_EXTRA: u8 = 124;
const SYSTEM_STATUS_MIN_PAYLOAD_LEN: usize = 19;
const SCALED_IMU_MESSAGE_ID: u32 = 26;
const SCALED_IMU_CRC_EXTRA: u8 = 170;
const SCALED_IMU_MIN_PAYLOAD_LEN: usize = 22;
const GPS_RAW_INT_MESSAGE_ID: u32 = 24;
const GPS_RAW_INT_CRC_EXTRA: u8 = 24;
const GPS_RAW_INT_MIN_PAYLOAD_LEN: usize = 30;
const ATTITUDE_MESSAGE_ID: u32 = 30;
const ATTITUDE_CRC_EXTRA: u8 = 39;
const ATTITUDE_MIN_PAYLOAD_LEN: usize = 28;
const COMMAND_ACK_MESSAGE_ID: u32 = 77;
const COMMAND_ACK_CRC_EXTRA: u8 = 143;
const COMMAND_ACK_MIN_PAYLOAD_LEN: usize = 3;
const STATUS_TEXT_MESSAGE_ID: u32 = 253;
const STATUS_TEXT_CRC_EXTRA: u8 = 83;
const STATUS_TEXT_MIN_PAYLOAD_LEN: usize = 51;
const COMMAND_LONG_MESSAGE_ID: u8 = 76;
const COMMAND_LONG_CRC_EXTRA: u8 = 152;

pub const MAV_CMD_PREFLIGHT_CALIBRATION: u16 = 241;
pub const MAV_COMP_ID_GCS: u8 = 190;
pub const MAV_SYS_ID_GCS: u8 = 255;

/// Creates a MAVLink v1 COMMAND_LONG frame.
///
/// MAVLink v1 is intentionally used for outbound commands because both
/// ArduPilot and PX4 accept it on standard serial telemetry links.
pub fn command_long_v1(
    sequence: u8,
    target_system: u8,
    target_component: u8,
    command: u16,
    confirmation: u8,
    params: [f32; 7],
) -> Vec<u8> {
    const PAYLOAD_LEN: usize = 33;
    let mut frame = Vec::with_capacity(6 + PAYLOAD_LEN + 2);
    frame.extend_from_slice(&[
        MAVLINK_V1_STX,
        PAYLOAD_LEN as u8,
        sequence,
        MAV_SYS_ID_GCS,
        MAV_COMP_ID_GCS,
        COMMAND_LONG_MESSAGE_ID,
    ]);

    for param in params {
        frame.extend_from_slice(&param.to_le_bytes());
    }
    frame.extend_from_slice(&command.to_le_bytes());
    frame.extend_from_slice(&[target_system, target_component, confirmation]);

    let checksum = x25_checksum(&frame[1..], COMMAND_LONG_CRC_EXTRA);
    frame.extend_from_slice(&checksum.to_le_bytes());
    frame
}

fn expected_frame_len(buffer: &[u8]) -> Option<usize> {
    let stx = *buffer.first()?;
    let payload_len = *buffer.get(1)? as usize;

    match stx {
        MAVLINK_V1_STX => Some(6 + payload_len + 2),
        MAVLINK_V2_STX => {
            let incompat_flags = *buffer.get(2)?;
            let signature_len = if incompat_flags & MAVLINK_V2_SIGNED_FLAG != 0 {
                13
            } else {
                0
            };

            Some(10 + payload_len + 2 + signature_len)
        }
        _ => None,
    }
}

fn parse_mavlink_frame(frame: &[u8]) -> Option<MavlinkMessage> {
    let (message_id, system_id, component_id, payload) = match frame.first().copied()? {
        MAVLINK_V1_STX => {
            let payload_len = *frame.get(1)? as usize;
            let message_id = u32::from(*frame.get(5)?);
            let crc_extra = message_crc_extra(message_id)?;
            let frame_len = 6 + payload_len + 2;

            if frame.len() != frame_len || !valid_v1_checksum(frame, crc_extra) {
                return None;
            }

            (message_id, frame[3], frame[4], &frame[6..6 + payload_len])
        }
        MAVLINK_V2_STX => {
            let payload_len = *frame.get(1)? as usize;
            let message_id =
                u32::from(frame[7]) | (u32::from(frame[8]) << 8) | (u32::from(frame[9]) << 16);
            let crc_extra = message_crc_extra(message_id)?;
            let signature_len = if frame[2] & MAVLINK_V2_SIGNED_FLAG != 0 {
                13
            } else {
                0
            };
            let frame_len = 10 + payload_len + 2 + signature_len;

            if frame.len() != frame_len || !valid_v2_checksum(frame, payload_len, crc_extra) {
                return None;
            }

            (message_id, frame[5], frame[6], &frame[10..10 + payload_len])
        }
        _ => return None,
    };

    match message_id {
        HEARTBEAT_MESSAGE_ID if payload.len() >= HEARTBEAT_MIN_PAYLOAD_LEN => {
            Some(MavlinkMessage::Heartbeat(Heartbeat {
                system_id,
                component_id,
                autopilot: payload[5],
            }))
        }
        SYSTEM_STATUS_MESSAGE_ID if payload.len() >= SYSTEM_STATUS_MIN_PAYLOAD_LEN => {
            Some(MavlinkMessage::SystemStatus(SystemStatus {
                sensors_present: read_u32(payload, 0)?,
                sensors_enabled: read_u32(payload, 4)?,
                sensors_health: read_u32(payload, 8)?,
                voltage_battery_mv: read_u16(payload, 14)?,
                current_battery_ca: read_i16(payload, 16)?,
                battery_remaining_percent: *payload.get(18)? as i8,
            }))
        }
        SCALED_IMU_MESSAGE_ID if payload.len() >= SCALED_IMU_MIN_PAYLOAD_LEN => {
            Some(MavlinkMessage::ScaledImu(ScaledImu {
                time_boot_ms: read_u32(payload, 0)?,
                xacc_mg: read_i16(payload, 4)?,
                yacc_mg: read_i16(payload, 6)?,
                zacc_mg: read_i16(payload, 8)?,
                xgyro_mrad_s: read_i16(payload, 10)?,
                ygyro_mrad_s: read_i16(payload, 12)?,
                zgyro_mrad_s: read_i16(payload, 14)?,
                xmag_mgauss: read_i16(payload, 16)?,
                ymag_mgauss: read_i16(payload, 18)?,
                zmag_mgauss: read_i16(payload, 20)?,
            }))
        }
        GPS_RAW_INT_MESSAGE_ID if payload.len() >= GPS_RAW_INT_MIN_PAYLOAD_LEN => {
            Some(MavlinkMessage::GpsRawInt(GpsRawInt {
                latitude_e7: read_i32(payload, 8)?,
                longitude_e7: read_i32(payload, 12)?,
                altitude_mm: read_i32(payload, 16)?,
                horizontal_accuracy_cm: read_u16(payload, 20)?,
                vertical_accuracy_cm: read_u16(payload, 22)?,
                velocity_cm_s: read_u16(payload, 24)?,
                course_over_ground_cdeg: read_u16(payload, 26)?,
                fix_type: *payload.get(28)?,
                satellites_visible: *payload.get(29)?,
            }))
        }
        ATTITUDE_MESSAGE_ID if payload.len() >= ATTITUDE_MIN_PAYLOAD_LEN => {
            Some(MavlinkMessage::Attitude(Attitude {
                time_boot_ms: read_u32(payload, 0)?,
                roll_rad: read_f32(payload, 4)?,
                pitch_rad: read_f32(payload, 8)?,
                yaw_rad: read_f32(payload, 12)?,
                rollspeed_rad_s: read_f32(payload, 16)?,
                pitchspeed_rad_s: read_f32(payload, 20)?,
                yawspeed_rad_s: read_f32(payload, 24)?,
            }))
        }
        COMMAND_ACK_MESSAGE_ID if payload.len() >= COMMAND_ACK_MIN_PAYLOAD_LEN => {
            Some(MavlinkMessage::CommandAck(CommandAck {
                command: read_u16(payload, 0)?,
                result: *payload.get(2)?,
                progress: payload.get(3).copied(),
            }))
        }
        STATUS_TEXT_MESSAGE_ID if payload.len() >= STATUS_TEXT_MIN_PAYLOAD_LEN => {
            let text_bytes = &payload[1..STATUS_TEXT_MIN_PAYLOAD_LEN];
            let text_end = text_bytes
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(text_bytes.len());
            Some(MavlinkMessage::StatusText(StatusText {
                severity: payload[0],
                text: String::from_utf8_lossy(&text_bytes[..text_end]).into_owned(),
            }))
        }
        _ => None,
    }
}

fn message_crc_extra(message_id: u32) -> Option<u8> {
    match message_id {
        HEARTBEAT_MESSAGE_ID => Some(HEARTBEAT_CRC_EXTRA),
        SYSTEM_STATUS_MESSAGE_ID => Some(SYSTEM_STATUS_CRC_EXTRA),
        SCALED_IMU_MESSAGE_ID => Some(SCALED_IMU_CRC_EXTRA),
        GPS_RAW_INT_MESSAGE_ID => Some(GPS_RAW_INT_CRC_EXTRA),
        ATTITUDE_MESSAGE_ID => Some(ATTITUDE_CRC_EXTRA),
        COMMAND_ACK_MESSAGE_ID => Some(COMMAND_ACK_CRC_EXTRA),
        STATUS_TEXT_MESSAGE_ID => Some(STATUS_TEXT_CRC_EXTRA),
        _ => None,
    }
}

fn read_u16(payload: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        payload.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn read_i16(payload: &[u8], offset: usize) -> Option<i16> {
    Some(i16::from_le_bytes(
        payload.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn read_u32(payload: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        payload.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn read_i32(payload: &[u8], offset: usize) -> Option<i32> {
    Some(i32::from_le_bytes(
        payload.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn read_f32(payload: &[u8], offset: usize) -> Option<f32> {
    Some(f32::from_le_bytes(
        payload.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn valid_v1_checksum(frame: &[u8], crc_extra: u8) -> bool {
    let payload_len = frame[1] as usize;
    let checksum_start = 6 + payload_len;
    let expected = u16::from_le_bytes([frame[checksum_start], frame[checksum_start + 1]]);
    let actual = x25_checksum(&frame[1..checksum_start], crc_extra);

    actual == expected
}

fn valid_v2_checksum(frame: &[u8], payload_len: usize, crc_extra: u8) -> bool {
    let checksum_start = 10 + payload_len;
    let expected = u16::from_le_bytes([frame[checksum_start], frame[checksum_start + 1]]);
    let actual = x25_checksum(&frame[1..checksum_start], crc_extra);

    actual == expected
}

fn x25_checksum(data: &[u8], crc_extra: u8) -> u16 {
    let mut crc = 0xffff;

    for byte in data.iter().copied().chain([crc_extra]) {
        let tmp = byte ^ (crc as u8);
        let tmp = tmp ^ (tmp << 4);
        crc = (crc >> 8) ^ (u16::from(tmp) << 8) ^ (u16::from(tmp) << 3) ^ (u16::from(tmp) >> 4);
    }

    crc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn append_checksum_v1(mut frame: Vec<u8>, crc_extra: u8) -> Vec<u8> {
        let payload_len = frame[1] as usize;
        let checksum_start = 6 + payload_len;
        let checksum = x25_checksum(&frame[1..checksum_start], crc_extra);
        frame.extend_from_slice(&checksum.to_le_bytes());
        frame
    }

    fn append_checksum_v2(mut frame: Vec<u8>, crc_extra: u8) -> Vec<u8> {
        let payload_len = frame[1] as usize;
        let checksum_start = 10 + payload_len;
        let checksum = x25_checksum(&frame[1..checksum_start], crc_extra);
        frame.extend_from_slice(&checksum.to_le_bytes());
        frame
    }

    #[test]
    fn parses_v1_ardupilot_heartbeat() {
        let frame = append_checksum_v1(
            vec![MAVLINK_V1_STX, 9, 1, 42, 1, 0, 0, 0, 0, 0, 2, 3, 81, 4, 3],
            HEARTBEAT_CRC_EXTRA,
        );

        let mut parser = MavlinkParser::new();
        let messages = parser.push(&frame);

        assert_eq!(
            messages,
            vec![MavlinkMessage::Heartbeat(Heartbeat {
                system_id: 42,
                component_id: 1,
                autopilot: 3,
            })]
        );
    }

    #[test]
    fn parses_v2_px4_heartbeat_after_noise_and_split_chunks() {
        let frame = append_checksum_v2(
            vec![
                MAVLINK_V2_STX,
                9,
                0,
                0,
                7,
                1,
                1,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                2,
                12,
                81,
                4,
                3,
            ],
            HEARTBEAT_CRC_EXTRA,
        );

        let mut parser = MavlinkParser::new();
        assert!(parser.push(&[0, 1, 2, frame[0], frame[1]]).is_empty());
        let messages = parser.push(&frame[2..]);

        assert_eq!(
            messages,
            vec![MavlinkMessage::Heartbeat(Heartbeat {
                system_id: 1,
                component_id: 1,
                autopilot: 12,
            })]
        );
    }

    #[test]
    fn ignores_heartbeat_with_bad_checksum() {
        let mut frame = append_checksum_v1(
            vec![MAVLINK_V1_STX, 9, 1, 1, 1, 0, 0, 0, 0, 0, 2, 3, 81, 4, 3],
            HEARTBEAT_CRC_EXTRA,
        );
        let last = frame.len() - 1;
        frame[last] ^= 0xff;

        let mut parser = MavlinkParser::new();

        assert!(parser.push(&frame).is_empty());
    }

    #[test]
    fn parses_system_status_sensor_health_and_battery() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&0x3f_u32.to_le_bytes());
        payload.extend_from_slice(&0x3f_u32.to_le_bytes());
        payload.extend_from_slice(&0x37_u32.to_le_bytes());
        payload.extend_from_slice(&250_u16.to_le_bytes());
        payload.extend_from_slice(&15_240_u16.to_le_bytes());
        payload.extend_from_slice(&321_i16.to_le_bytes());
        payload.push(78);
        payload.extend_from_slice(&[0; 12]);

        let mut frame = vec![MAVLINK_V1_STX, payload.len() as u8, 9, 1, 1, 1];
        frame.extend_from_slice(&payload);
        let frame = append_checksum_v1(frame, SYSTEM_STATUS_CRC_EXTRA);

        let messages = MavlinkParser::new().push(&frame);

        assert_eq!(
            messages,
            vec![MavlinkMessage::SystemStatus(SystemStatus {
                sensors_present: 0x3f,
                sensors_enabled: 0x3f,
                sensors_health: 0x37,
                voltage_battery_mv: 15_240,
                current_battery_ca: 321,
                battery_remaining_percent: 78,
            })]
        );
    }

    #[test]
    fn parses_scaled_imu_readings() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&42_u32.to_le_bytes());
        for value in [100_i16, -200, 980, 11, -12, 13, 210, -220, 230] {
            payload.extend_from_slice(&value.to_le_bytes());
        }

        let mut frame = vec![MAVLINK_V1_STX, payload.len() as u8, 3, 1, 1, 26];
        frame.extend_from_slice(&payload);
        let frame = append_checksum_v1(frame, SCALED_IMU_CRC_EXTRA);

        let messages = MavlinkParser::new().push(&frame);

        assert_eq!(
            messages,
            vec![MavlinkMessage::ScaledImu(ScaledImu {
                time_boot_ms: 42,
                xacc_mg: 100,
                yacc_mg: -200,
                zacc_mg: 980,
                xgyro_mrad_s: 11,
                ygyro_mrad_s: -12,
                zgyro_mrad_s: 13,
                xmag_mgauss: 210,
                ymag_mgauss: -220,
                zmag_mgauss: 230,
            })]
        );
    }

    #[test]
    fn parses_attitude_readings() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&123_u32.to_le_bytes());
        for value in [0.1_f32, -0.2, 0.3, 0.4, -0.5, 0.6] {
            payload.extend_from_slice(&value.to_le_bytes());
        }

        let mut frame = vec![MAVLINK_V1_STX, payload.len() as u8, 3, 1, 1, 30];
        frame.extend_from_slice(&payload);
        let frame = append_checksum_v1(frame, ATTITUDE_CRC_EXTRA);

        let messages = MavlinkParser::new().push(&frame);

        assert_eq!(
            messages,
            vec![MavlinkMessage::Attitude(Attitude {
                time_boot_ms: 123,
                roll_rad: 0.1,
                pitch_rad: -0.2,
                yaw_rad: 0.3,
                rollspeed_rad_s: 0.4,
                pitchspeed_rad_s: -0.5,
                yawspeed_rad_s: 0.6,
            })]
        );
    }

    #[test]
    fn parses_gps_raw_int_readings() {
        let mut payload = vec![0_u8; GPS_RAW_INT_MIN_PAYLOAD_LEN];
        payload[8..12].copy_from_slice(&12_345_678_i32.to_le_bytes());
        payload[12..16].copy_from_slice(&87_654_321_i32.to_le_bytes());
        payload[16..20].copy_from_slice(&543_210_i32.to_le_bytes());
        payload[20..22].copy_from_slice(&125_u16.to_le_bytes());
        payload[22..24].copy_from_slice(&250_u16.to_le_bytes());
        payload[24..26].copy_from_slice(&345_u16.to_le_bytes());
        payload[26..28].copy_from_slice(&6_789_u16.to_le_bytes());
        payload[28] = 3;
        payload[29] = 14;

        let mut frame = vec![MAVLINK_V1_STX, payload.len() as u8, 4, 1, 1, 24];
        frame.extend_from_slice(&payload);
        let frame = append_checksum_v1(frame, GPS_RAW_INT_CRC_EXTRA);

        let messages = MavlinkParser::new().push(&frame);

        assert_eq!(
            messages,
            vec![MavlinkMessage::GpsRawInt(GpsRawInt {
                fix_type: 3,
                satellites_visible: 14,
                latitude_e7: 12_345_678,
                longitude_e7: 87_654_321,
                altitude_mm: 543_210,
                horizontal_accuracy_cm: 125,
                vertical_accuracy_cm: 250,
                velocity_cm_s: 345,
                course_over_ground_cdeg: 6_789,
            })]
        );
    }

    #[test]
    fn parses_command_ack_and_status_text() {
        let command_ack = append_checksum_v1(
            vec![MAVLINK_V1_STX, 3, 5, 1, 1, 77, 241, 0, 5],
            COMMAND_ACK_CRC_EXTRA,
        );
        let mut status_payload = vec![6_u8];
        status_payload.extend_from_slice(b"Calibration started");
        status_payload.resize(STATUS_TEXT_MIN_PAYLOAD_LEN, 0);
        let mut status_text = vec![MAVLINK_V1_STX, status_payload.len() as u8, 6, 1, 1, 253];
        status_text.extend_from_slice(&status_payload);
        let status_text = append_checksum_v1(status_text, STATUS_TEXT_CRC_EXTRA);

        let mut parser = MavlinkParser::new();
        let mut messages = parser.push(&command_ack);
        messages.extend(parser.push(&status_text));

        assert_eq!(
            messages,
            vec![
                MavlinkMessage::CommandAck(CommandAck {
                    command: 241,
                    result: 5,
                    progress: None,
                }),
                MavlinkMessage::StatusText(StatusText {
                    severity: 6,
                    text: "Calibration started".to_string(),
                }),
            ]
        );
    }

    #[test]
    fn encodes_accelerometer_preflight_calibration_command() {
        let frame = command_long_v1(
            8,
            1,
            1,
            MAV_CMD_PREFLIGHT_CALIBRATION,
            0,
            [0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        );

        assert_eq!(frame.len(), 41);
        assert_eq!(&frame[..6], &[MAVLINK_V1_STX, 33, 8, 255, 190, 76]);
        assert_eq!(f32::from_le_bytes(frame[22..26].try_into().unwrap()), 1.0);
        assert_eq!(u16::from_le_bytes(frame[34..36].try_into().unwrap()), 241);
        assert_eq!(&frame[36..39], &[1, 1, 0]);

        let expected_checksum = x25_checksum(&frame[1..39], COMMAND_LONG_CRC_EXTRA);
        assert_eq!(
            u16::from_le_bytes(frame[39..41].try_into().unwrap()),
            expected_checksum
        );
    }
}
