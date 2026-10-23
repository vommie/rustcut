use serde_json::json;

use super::*;
use crate::media::model::FrameRateMode;

fn decode_value(value: &serde_json::Value) -> Result<MediaInfo, DecodeError> {
    decode(value.to_string().as_bytes())
}

fn video_of(info: &MediaInfo) -> &VideoStream {
    info.primary_video().unwrap().1
}

#[test]
fn a_typical_mp4() {
    let info = decode_value(&json!({
        "streams": [
            {
                "index": 0, "codec_type": "video", "codec_name": "h264",
                "codec_long_name": "H.264 / AVC / MPEG-4 AVC / MPEG-4 part 10", "profile": "High",
                "width": 1920, "height": 1080, "sample_aspect_ratio": "1:1", "display_aspect_ratio": "16:9",
                "pix_fmt": "yuv420p", "field_order": "progressive", "r_frame_rate": "30000/1001",
                "avg_frame_rate": "30000/1001", "start_time": "0.000000", "duration": "10.010000",
                "bit_rate": "8000000", "bits_per_raw_sample": "8", "nb_frames": "300",
                "color_range": "tv", "color_space": "bt709", "color_transfer": "bt709", "color_primaries": "bt709",
                "disposition": {"default": 1, "attached_pic": 0},
                "tags": {"language": "und", "handler_name": "VideoHandler"}
            },
            {
                "index": 1, "codec_type": "audio", "codec_name": "aac", "profile": "LC", "sample_fmt": "fltp",
                "sample_rate": "48000", "channels": 2, "channel_layout": "stereo", "bit_rate": "128000",
                "duration": "10.005333", "disposition": {"default": 1}, "tags": {"language": "eng"}
            }
        ],
        "format": {
            "format_name": "mov,mp4,m4a,3gp,3g2,mj2", "format_long_name": "QuickTime / MOV",
            "start_time": "0.000000", "duration": "10.010000", "size": "10000000", "bit_rate": "7992007",
            "tags": {"title": "Holiday"}
        }
    }))
    .unwrap();
    assert_eq!(info.container.format_long.as_deref(), Some("QuickTime / MOV"));
    assert_eq!(info.container.duration, Some(Timestamp::from_millis(10_010)));
    assert_eq!(info.container.size, Some(10_000_000));
    assert_eq!(info.container.bit_rate, Some(7_992_007));
    assert_eq!(info.container.title.as_deref(), Some("Holiday"));
    let (stream, video) = info.primary_video().unwrap();
    assert_eq!(stream.codec.as_deref(), Some("h264"));
    assert_eq!(stream.language, None, "`und` is unknown");
    assert!(stream.default);
    assert_eq!((video.width, video.height), (Some(1920), Some(1080)));
    assert_eq!(video.frame_rate, FrameRate::new(30000, 1001));
    assert_eq!(video.bit_depth, Some(8));
    assert_eq!(video.frame_count, Some(300));
    assert_eq!(video.color.primaries.as_deref(), Some("bt709"));
    assert!(!video.is_interlaced() && !video.is_anamorphic());
    let (audio_stream, audio) = info.audio_streams().next().unwrap();
    assert_eq!(audio_stream.language.as_deref(), Some("eng"));
    assert_eq!((audio.sample_rate, audio.channels), (Some(48_000), Some(2)));
    assert_eq!(audio.channel_layout.as_deref(), Some("stereo"));
    assert_eq!(info.frame_timing, None, "no packets were asked for");
    assert_eq!(info.frame_rate_mode(), FrameRateMode::Constant);
}

#[test]
fn rotation_follows_mpv() {
    let with_side_data = |rotation: serde_json::Value| {
        let info = decode_value(&json!({"streams": [{
            "index": 0, "codec_type": "video", "width": 640, "height": 360,
            "side_data_list": [{"side_data_type": "CPB properties"}, {"side_data_type": "Display Matrix", "rotation": rotation}]
        }]}))
        .unwrap();
        video_of(&info).rotation
    };
    // ffprobe's rotation is counter-clockwise, mpv rotates clockwise.
    assert_eq!(with_side_data(json!(90)), 270);
    assert_eq!(with_side_data(json!(-90)), 90);
    assert_eq!(with_side_data(json!(180)), 180);
    assert_eq!(with_side_data(json!(-180)), 180);
    assert_eq!(with_side_data(json!("-90.0")), 90);
    assert_eq!(with_side_data(json!("garbage")), 0);
    // The old `rotate` tag is clockwise already.
    let info = decode_value(&json!({"streams": [{"codec_type": "video", "tags": {"rotate": "90"}}]})).unwrap();
    assert_eq!(video_of(&info).rotation, 90);
}

#[test]
fn matroska_durations_and_bit_rates_come_from_tags() {
    let info = decode_value(&json!({"streams": [
        {"index": 0, "codec_type": "video", "tags": {"DURATION": "00:00:08.000000000", "BPS": "1048576"}},
        {"index": 1, "codec_type": "audio", "tags": {"DURATION-eng": "00:01:02.021000000", "LANGUAGE": "deu"}}
    ]}))
    .unwrap();
    let video = info.streams.first().unwrap();
    assert_eq!(video.duration, Some(Timestamp::from_secs(8)));
    assert_eq!(video.bit_rate, Some(1_048_576));
    let audio = info.streams.get(1).unwrap();
    assert_eq!(audio.duration, Some(Timestamp::from_millis(62_021)));
    assert_eq!(audio.language.as_deref(), Some("deu"));
    assert_eq!(
        info.duration(),
        Some(Timestamp::from_millis(62_021)),
        "the longest stream without a container duration"
    );
}

#[test]
fn broken_fields_become_unknown_instead_of_failing() {
    let info = decode_value(&json!({
        "streams": [{
            "index": "zero", "codec_type": "video", "codec_name": 264, "width": "N/A", "height": -5,
            "r_frame_rate": "0/0", "avg_frame_rate": "abc", "sample_aspect_ratio": "0:1", "bit_rate": "N/A",
            "duration": "N/A", "nb_frames": {"x": 1}, "disposition": [1, 2], "tags": "not a map",
            "side_data_list": 7, "unknown_future_field": {"a": [1, 2, 3]}
        }],
        "format": {"duration": null, "size": "huge", "format_name": ""},
        "packets": "nope"
    }))
    .unwrap();
    let stream = info.streams.first().unwrap();
    assert_eq!(stream.index, 0, "falls back to the position");
    assert_eq!(stream.codec, None);
    assert_eq!(stream.bit_rate, None);
    assert_eq!(stream.duration, None);
    let video = video_of(&info);
    assert_eq!((video.width, video.height), (None, None));
    assert_eq!(video.frame_rate, None);
    assert_eq!(video.sample_aspect, None);
    assert_eq!(video.frame_count, None);
    assert_eq!(info.container.format, None);
    assert_eq!(info.container.size, None);
    assert_eq!(info.frame_rate_mode(), FrameRateMode::Unknown);
}

#[test]
fn negative_start_times_and_cover_art() {
    let info = decode_value(&json!({
        "streams": [
            {"index": 0, "codec_type": "audio", "start_time": "-0.021333"},
            {"index": 1, "codec_type": "video", "codec_name": "mjpeg", "disposition": {"attached_pic": 1}}
        ],
        "format": {"start_time": "-0.021333"}
    }))
    .unwrap();
    assert_eq!(info.container.start_time, Some(Timestamp::from_micros(-21_333)));
    assert_eq!(
        info.streams.first().unwrap().start_time,
        Some(Timestamp::from_micros(-21_333))
    );
    assert!(info.primary_video().is_none(), "cover art is not a video track");
    assert!(info.has_audio());
}

#[test]
fn stream_kinds() {
    let info = decode_value(&json!({"streams": [
        {"codec_type": "subtitle"}, {"codec_type": "data"}, {"codec_type": "attachment"}, {"codec_type": "hologram"}, {}
    ]}))
    .unwrap();
    let kinds: Vec<&str> = info.streams.iter().map(|s| s.kind.name()).collect();
    assert_eq!(kinds, ["subtitle", "data", "attachment", "hologram", "unknown"]);
    let indices: Vec<u32> = info.streams.iter().map(|s| s.index).collect();
    assert_eq!(indices, [0, 1, 2, 3, 4]);
}

#[test]
fn packets_of_the_primary_video_give_the_frame_timing() {
    let mut packets: Vec<serde_json::Value> = (0..60)
        .map(|i| json!({"stream_index": 0, "pts_time": format!("{:.6}", f64::from(i) / 30.0)}))
        .collect();
    packets.extend((0..20).map(|i| json!({"stream_index": 0, "pts_time": 2.0 + f64::from(i) * 0.1})));
    // Audio packets in between are ignored.
    packets.extend((0..50).map(|i| json!({"stream_index": 1, "pts_time": f64::from(i) * 0.021})));
    let info = decode_value(&json!({
        "streams": [
            {"index": 0, "codec_type": "video", "r_frame_rate": "30/1", "avg_frame_rate": "30/1"},
            {"index": 1, "codec_type": "audio"}
        ],
        "packets": packets
    }))
    .unwrap();
    let timing = info.frame_timing.unwrap();
    assert_eq!(timing.frames, 80);
    assert!(timing.variable);
    assert!(matches!(
        info.frame_rate_mode(),
        FrameRateMode::Variable {
            min_fps: Some(_),
            max_fps: Some(_)
        }
    ));
}

#[test]
fn reported_errors_and_garbage() {
    assert_eq!(
        decode_value(&json!({"error": {"code": -1094995529, "string": "Invalid data found when processing input"}})),
        Err(DecodeError::Reported("Invalid data found when processing input".into()))
    );
    assert_eq!(
        decode_value(&json!({"error": {}})),
        Err(DecodeError::Reported("ffprobe could not read the file".into()))
    );
    assert!(matches!(decode(b"not json"), Err(DecodeError::Malformed(_))));
    assert!(matches!(decode(b""), Err(DecodeError::Malformed(_))));
    // What ffprobe prints for a file it cannot open when -show_error is missing.
    let empty = decode(b"{\n\n}").unwrap();
    assert!(empty.streams.is_empty() && empty.container == Container::default());
}

#[test]
fn bit_depth_from_the_pixel_format() {
    let cases = [
        ("yuv420p", Some(8)),
        ("yuvj420p", Some(8)),
        ("nv12", Some(8)),
        ("rgb24", Some(8)),
        ("yuv420p10le", Some(10)),
        ("yuv422p10be", Some(10)),
        ("yuv444p12le", Some(12)),
        ("gbrp10le", Some(10)),
        ("p010le", Some(10)),
        ("p016le", Some(16)),
        ("gray10le", Some(10)),
        ("gray", Some(8)),
        ("pal8", None),
        ("rgb48le", None),
        ("x2rgb10le", None),
        ("", None),
    ];
    for (name, depth) in cases {
        assert_eq!(bit_depth_of(name), depth, "{name}");
    }
    let stated = decode_value(
        &json!({"streams": [{"codec_type": "video", "pix_fmt": "yuv420p10le", "bits_per_raw_sample": "12"}]}),
    )
    .unwrap();
    assert_eq!(video_of(&stated).bit_depth, Some(12), "the stream's own statement wins");
}

#[test]
fn ratios_and_field_orders() {
    assert_eq!(ratio("16:9"), Ratio::new(16, 9));
    assert_eq!(ratio("0:1"), None);
    assert_eq!(ratio("N/A"), None);
    assert_eq!(ratio("4:"), None);
    let info = decode_value(
        &json!({"streams": [{"codec_type": "video", "field_order": "tt", "sample_aspect_ratio": "64:45"}]}),
    )
    .unwrap();
    let video = video_of(&info);
    assert_eq!(video.field_order, Some(FieldOrder::TopFirst));
    assert!(video.is_interlaced() && video.is_anamorphic());
    assert_eq!(FieldOrder::from_ffprobe("unknown"), None);
}
