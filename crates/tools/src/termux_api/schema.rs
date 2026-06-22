//! TermuxApiTool 的 JSON Schema 构造器。
//!
//! 用程序化方式（而非 `json!` 宏）构建庞大的参数 schema，避免宏递归深度限制。
//! 从 `termux_api.rs` 抽出以缩小主文件。

use serde_json::{json, Value};

fn prop_str(desc: &str) -> Value {
    json!({"type": "string", "description": desc})
}

fn prop_str_enum(desc: &str, values: &[&str]) -> Value {
    json!({"type": "string", "enum": values, "description": desc})
}

fn prop_int(desc: &str) -> Value {
    json!({"type": "integer", "description": desc})
}

fn prop_num(desc: &str) -> Value {
    json!({"type": "number", "description": desc})
}

fn prop_bool(desc: &str) -> Value {
    json!({"type": "boolean", "description": desc})
}

pub(super) fn build_schema() -> Value {
    use serde_json::Map;

    let mut props = Map::new();

    // action enum
    props.insert(
        "action".into(),
        prop_str_enum(
            "Termux API action to perform",
            &[
                "battery_status",
                "brightness",
                "camera_info",
                "camera_photo",
                "clipboard_get",
                "clipboard_set",
                "contact_list",
                "call_log",
                "dialog",
                "download",
                "fingerprint",
                "infrared_frequencies",
                "infrared_transmit",
                "keystore",
                "location",
                "media_player",
                "media_scan",
                "microphone_record",
                "notification",
                "notification_remove",
                "open",
                "open_url",
                "sensor",
                "share",
                "sms_list",
                "sms_send",
                "speech_to_text",
                "storage_get",
                "telephony_deviceinfo",
                "telephony_cellinfo",
                "telephony_call",
                "toast",
                "torch",
                "tts_engines",
                "tts_speak",
                "vibrate",
                "volume",
                "wallpaper",
                "wifi_connectioninfo",
                "wifi_scaninfo",
                "wifi_enable",
                "audio_info",
                "wake_lock",
                "wake_unlock",
                "job_scheduler",
                "info",
            ],
        ),
    );

    // General params
    props.insert("text".into(), prop_str("Text content for: clipboard_set, toast, tts_speak, sms_send, notification (--content), share (stdin text), dialog title"));
    props.insert("number".into(), prop_str("(sms_send, telephony_call) Phone number(s). For SMS, comma-separated for multiple recipients"));
    props.insert(
        "output_path".into(),
        prop_str("(camera_photo, microphone_record, storage_get) Output file path"),
    );
    props.insert(
        "camera_id".into(),
        prop_int("(camera_photo) Camera ID, default 0. Use camera_info to list cameras"),
    );
    props.insert(
        "brightness".into(),
        prop_int("(brightness) Screen brightness 0-255, or -1 for auto"),
    );
    props.insert(
        "title".into(),
        prop_str("(notification, download, share) Title text"),
    );
    props.insert(
        "url".into(),
        prop_str("(open_url, download, wallpaper) URL to open/download/set as wallpaper"),
    );
    props.insert(
        "file_path".into(),
        prop_str(
            "(open, share, media_scan, wallpaper) File path to open/share/scan/set as wallpaper",
        ),
    );
    props.insert(
        "content_type".into(),
        prop_str("(open, share) MIME content type"),
    );
    props.insert(
        "limit".into(),
        prop_int("(sms_list, call_log) Max number of items. Default: 10"),
    );
    props.insert(
        "offset".into(),
        prop_int("(sms_list, call_log) Offset in list. Default: 0"),
    );
    props.insert("duration".into(), prop_int("(vibrate) Duration in ms, default 1000. (microphone_record) Recording limit in seconds"));
    props.insert(
        "enabled".into(),
        prop_bool("(wifi_enable, torch) true=on, false=off"),
    );
    props.insert(
        "force".into(),
        prop_bool("(vibrate) Force vibration even in silent mode"),
    );
    props.insert(
        "recursive".into(),
        prop_bool("(media_scan) Scan directories recursively"),
    );

    // Location
    props.insert(
        "provider".into(),
        prop_str_enum(
            "(location) Location provider. Default: gps",
            &["gps", "network", "passive"],
        ),
    );
    props.insert(
        "request".into(),
        prop_str_enum(
            "(location) Request type. Default: once",
            &["once", "last", "updates"],
        ),
    );

    // Notification
    props.insert(
        "notification_id".into(),
        prop_str("(notification, notification_remove) Notification ID"),
    );
    props.insert(
        "priority".into(),
        prop_str_enum(
            "(notification) Notification priority",
            &["high", "low", "max", "min", "default"],
        ),
    );
    props.insert(
        "sound".into(),
        prop_bool("(notification) Play sound with notification"),
    );
    props.insert(
        "vibrate_pattern".into(),
        prop_str("(notification) Vibrate pattern, comma-separated ms values e.g. '500,1000,200'"),
    );
    props.insert(
        "led_color".into(),
        prop_str("(notification) LED color as RRGGBB hex"),
    );
    props.insert(
        "notification_action".into(),
        prop_str("(notification) Action to execute when pressing the notification"),
    );

    // Volume / stream
    props.insert(
        "stream".into(),
        prop_str_enum(
            "(volume, tts_speak) Audio stream",
            &["alarm", "music", "notification", "ring", "system", "call"],
        ),
    );
    props.insert(
        "volume_value".into(),
        prop_int("(volume) Volume level to set"),
    );

    // Share
    props.insert(
        "share_action".into(),
        prop_str_enum(
            "(share) Action to perform on shared content. Default: view",
            &["edit", "send", "view"],
        ),
    );

    // SMS
    props.insert(
        "sms_type".into(),
        prop_str_enum(
            "(sms_list) Type of SMS messages to list. Default: inbox",
            &["all", "inbox", "sent", "draft", "outbox"],
        ),
    );

    // Dialog
    props.insert(
        "dialog_widget".into(),
        prop_str_enum(
            "(dialog) Widget type for user input dialog",
            &[
                "confirm", "checkbox", "counter", "date", "radio", "sheet", "spinner", "speech",
                "text", "time",
            ],
        ),
    );
    props.insert(
        "dialog_values".into(),
        prop_str("(dialog) Comma-separated values for checkbox/radio/sheet/spinner widgets"),
    );

    // Sensor
    props.insert("sensor_name".into(), prop_str("(sensor) Sensor name(s) to listen to (partial match). Use 'list' to see available sensors"));
    props.insert(
        "sensor_limit".into(),
        prop_int("(sensor) Number of sensor readings to take. Default: 1"),
    );
    props.insert(
        "sensor_delay".into(),
        prop_int("(sensor) Delay between readings in ms"),
    );

    // TTS
    props.insert(
        "tts_engine".into(),
        prop_str("(tts_speak) TTS engine to use (see tts_engines)"),
    );
    props.insert(
        "tts_language".into(),
        prop_str("(tts_speak) Language code for TTS"),
    );
    props.insert(
        "tts_pitch".into(),
        prop_num("(tts_speak) Pitch multiplier, 1.0 is normal"),
    );
    props.insert(
        "tts_rate".into(),
        prop_num("(tts_speak) Speech rate multiplier, 1.0 is normal"),
    );

    // Microphone
    props.insert(
        "mic_action".into(),
        prop_str_enum(
            "(microphone_record) Recording action. Default: start",
            &["start", "info", "stop"],
        ),
    );
    props.insert(
        "encoder".into(),
        prop_str_enum(
            "(microphone_record) Audio encoder",
            &["aac", "amr_wb", "amr_nb"],
        ),
    );
    props.insert(
        "bitrate".into(),
        prop_int("(microphone_record) Recording bitrate in kbps"),
    );
    props.insert(
        "sample_rate".into(),
        prop_int("(microphone_record) Sampling rate in Hz"),
    );
    props.insert(
        "channels".into(),
        prop_int("(microphone_record) Channel count (1=mono, 2=stereo)"),
    );

    // Media player
    props.insert(
        "player_action".into(),
        prop_str_enum(
            "(media_player) Player action",
            &["play", "play_file", "pause", "stop", "info"],
        ),
    );

    // Infrared
    props.insert(
        "frequency".into(),
        prop_int("(infrared_transmit) IR carrier frequency in Hz"),
    );
    props.insert(
        "ir_pattern".into(),
        prop_str(
            "(infrared_transmit) IR on/off pattern, comma-separated intervals e.g. '20,50,20,30'",
        ),
    );

    // Keystore
    props.insert(
        "keystore_action".into(),
        prop_str_enum(
            "(keystore) Keystore operation",
            &["list", "generate", "delete", "sign", "verify"],
        ),
    );
    props.insert("key_alias".into(), prop_str("(keystore) Key alias name"));
    props.insert(
        "key_algorithm".into(),
        prop_str_enum(
            "(keystore generate) Algorithm. Default: RSA",
            &["RSA", "EC"],
        ),
    );
    props.insert(
        "key_size".into(),
        prop_int("(keystore generate) Key size. RSA: 2048/3072/4096. EC: 256/384/521"),
    );
    props.insert(
        "sign_algorithm".into(),
        prop_str("(keystore sign/verify) Signing algorithm e.g. 'SHA256withRSA'"),
    );
    props.insert(
        "sign_data".into(),
        prop_str("(keystore sign) Data to sign (passed via stdin)"),
    );
    props.insert(
        "signature".into(),
        prop_str("(keystore verify) Signature file path"),
    );

    // Wallpaper
    props.insert(
        "wallpaper_lockscreen".into(),
        prop_bool("(wallpaper) Set wallpaper for lockscreen (Android 7+)"),
    );

    // Toast
    props.insert(
        "toast_bg_color".into(),
        prop_str("(toast) Background color name or #RRGGBB"),
    );
    props.insert(
        "toast_text_color".into(),
        prop_str("(toast) Text color name or #RRGGBB"),
    );
    props.insert(
        "toast_position".into(),
        prop_str_enum(
            "(toast) Toast position. Default: middle",
            &["top", "middle", "bottom"],
        ),
    );
    props.insert(
        "toast_short".into(),
        prop_bool("(toast) Show toast for a short duration only"),
    );

    // Job scheduler
    props.insert(
        "job_script".into(),
        prop_str("(job_scheduler) Path to script to schedule"),
    );
    props.insert("job_id".into(), prop_int("(job_scheduler) Job ID"));
    props.insert(
        "job_period_ms".into(),
        prop_int("(job_scheduler) Repeat period in ms (0=once)"),
    );
    props.insert(
        "job_network".into(),
        prop_str_enum(
            "(job_scheduler) Required network type",
            &["any", "unmetered", "cellular", "not_roaming", "none"],
        ),
    );
    props.insert(
        "job_charging".into(),
        prop_bool("(job_scheduler) Run only when charging"),
    );
    props.insert(
        "job_list_pending".into(),
        prop_bool("(job_scheduler) List pending jobs instead of scheduling"),
    );

    let mut schema = Map::new();
    schema.insert("type".into(), json!("object"));
    schema.insert("properties".into(), Value::Object(props));
    schema.insert("required".into(), json!(["action"]));
    Value::Object(schema)
}
