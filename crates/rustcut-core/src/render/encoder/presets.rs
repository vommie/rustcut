//! Speed presets and tunes of the encoders, slowest first.

use super::Preset;

pub(super) const X26X_PRESETS: &[Preset] = &[
    Preset {
        value: "veryslow",
        label: "Very slow (best compression)",
    },
    Preset {
        value: "slower",
        label: "Slower",
    },
    Preset {
        value: "slow",
        label: "Slow",
    },
    Preset {
        value: "medium",
        label: "Medium (default)",
    },
    Preset {
        value: "fast",
        label: "Fast",
    },
    Preset {
        value: "faster",
        label: "Faster",
    },
    Preset {
        value: "veryfast",
        label: "Very fast",
    },
    Preset {
        value: "superfast",
        label: "Super fast",
    },
    Preset {
        value: "ultrafast",
        label: "Ultra fast (largest files)",
    },
];

pub(super) const SVT_PRESETS: &[Preset] = &[
    Preset {
        value: "0",
        label: "0 - Slowest (archival)",
    },
    Preset { value: "1", label: "1" },
    Preset {
        value: "2",
        label: "2 - Very slow",
    },
    Preset { value: "3", label: "3" },
    Preset {
        value: "4",
        label: "4 - High quality",
    },
    Preset {
        value: "5",
        label: "5 - Slow quality",
    },
    Preset {
        value: "6",
        label: "6 - Recommended balance",
    },
    Preset {
        value: "7",
        label: "7 - Fast",
    },
    Preset {
        value: "8",
        label: "8 - Faster",
    },
    Preset { value: "9", label: "9" },
    Preset {
        value: "10",
        label: "10 - Very fast",
    },
    Preset {
        value: "11",
        label: "11",
    },
    Preset {
        value: "12",
        label: "12 - Fastest",
    },
    Preset {
        value: "13",
        label: "13 - Real time",
    },
];

pub(super) const AOM_PRESETS: &[Preset] = &[
    Preset {
        value: "0",
        label: "0 - Slowest",
    },
    Preset { value: "1", label: "1" },
    Preset { value: "2", label: "2" },
    Preset { value: "3", label: "3" },
    Preset {
        value: "4",
        label: "4 - Balanced",
    },
    Preset { value: "5", label: "5" },
    Preset {
        value: "6",
        label: "6 - Fast",
    },
    Preset { value: "7", label: "7" },
    Preset {
        value: "8",
        label: "8 - Fastest",
    },
];

pub(super) const RAV1E_PRESETS: &[Preset] = &[
    Preset {
        value: "0",
        label: "0 - Slowest",
    },
    Preset { value: "2", label: "2" },
    Preset { value: "4", label: "4" },
    Preset {
        value: "6",
        label: "6 - Balanced (default)",
    },
    Preset { value: "8", label: "8" },
    Preset {
        value: "10",
        label: "10 - Fastest",
    },
];

pub(super) const NVENC_PRESETS: &[Preset] = &[
    Preset {
        value: "p7",
        label: "P7 - Slowest, best quality",
    },
    Preset {
        value: "p6",
        label: "P6",
    },
    Preset {
        value: "p5",
        label: "P5 - Slow",
    },
    Preset {
        value: "p4",
        label: "P4 - Medium (default)",
    },
    Preset {
        value: "p3",
        label: "P3",
    },
    Preset {
        value: "p2",
        label: "P2",
    },
    Preset {
        value: "p1",
        label: "P1 - Fastest",
    },
];

pub(super) const QSV_PRESETS: &[Preset] = &[
    Preset {
        value: "veryslow",
        label: "Very slow",
    },
    Preset {
        value: "slower",
        label: "Slower",
    },
    Preset {
        value: "slow",
        label: "Slow",
    },
    Preset {
        value: "medium",
        label: "Medium (default)",
    },
    Preset {
        value: "fast",
        label: "Fast",
    },
    Preset {
        value: "faster",
        label: "Faster",
    },
    Preset {
        value: "veryfast",
        label: "Very fast",
    },
];

pub(super) const SVT_TUNES: &[Preset] = &[
    Preset {
        value: "0",
        label: "Visual quality (VQ)",
    },
    Preset {
        value: "1",
        label: "PSNR (default)",
    },
    Preset {
        value: "2",
        label: "SSIM",
    },
];

pub(super) const X264_TUNES: &[Preset] = &[
    Preset {
        value: "film",
        label: "Film",
    },
    Preset {
        value: "animation",
        label: "Animation",
    },
    Preset {
        value: "grain",
        label: "Grain (keeps film grain)",
    },
    Preset {
        value: "stillimage",
        label: "Still image",
    },
    Preset {
        value: "fastdecode",
        label: "Fast decode",
    },
    Preset {
        value: "zerolatency",
        label: "Zero latency",
    },
];

pub(super) const X265_TUNES: &[Preset] = &[
    Preset {
        value: "grain",
        label: "Grain (keeps film grain)",
    },
    Preset {
        value: "animation",
        label: "Animation",
    },
    Preset {
        value: "fastdecode",
        label: "Fast decode",
    },
    Preset {
        value: "zerolatency",
        label: "Zero latency",
    },
];

pub(super) const AOM_TUNES: &[Preset] = &[
    Preset {
        value: "psnr",
        label: "PSNR",
    },
    Preset {
        value: "ssim",
        label: "SSIM",
    },
];

pub(super) const NVENC_TUNES: &[Preset] = &[
    Preset {
        value: "hq",
        label: "High quality (default)",
    },
    Preset {
        value: "uhq",
        label: "Ultra high quality",
    },
    Preset {
        value: "ll",
        label: "Low latency",
    },
];

pub(super) const NONE: &[Preset] = &[];

pub(super) const X264_AQ: &[Preset] = &[
    Preset {
        value: "0",
        label: "Off",
    },
    Preset {
        value: "1",
        label: "Variance (default)",
    },
    Preset {
        value: "2",
        label: "Auto-variance",
    },
    Preset {
        value: "3",
        label: "Auto-variance, dark scenes favored",
    },
];

pub(super) const X265_AQ: &[Preset] = &[
    Preset {
        value: "0",
        label: "Off",
    },
    Preset {
        value: "1",
        label: "Variance",
    },
    Preset {
        value: "2",
        label: "Auto-variance (default)",
    },
    Preset {
        value: "3",
        label: "Auto-variance, dark scenes favored",
    },
    Preset {
        value: "4",
        label: "Auto-variance with edges",
    },
];

pub(super) const SVT_AQ: &[Preset] = &[
    Preset {
        value: "0",
        label: "Off",
    },
    Preset {
        value: "1",
        label: "Variance",
    },
    Preset {
        value: "2",
        label: "Prediction efficiency (default)",
    },
];

pub(super) const AOM_AQ: &[Preset] = &[
    Preset {
        value: "0",
        label: "Off (default)",
    },
    Preset {
        value: "1",
        label: "Variance",
    },
    Preset {
        value: "2",
        label: "Complexity",
    },
    Preset {
        value: "3",
        label: "Cyclic refresh",
    },
];
