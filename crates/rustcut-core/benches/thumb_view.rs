//! What it costs to show one thumbnail: decoding a stored picture, then the crop and turn that make it look like the big
//! picture. Only the picture on screen goes through this, so a changed crop or rotation costs nothing up front.

#![allow(clippy::unwrap_used, reason = "a benchmark stops at the first thing that fails")]

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use rustcut_core::edit::{Crop, Orientation, Size};
use rustcut_core::thumbs::{ThumbView, render};

fn stored() -> Vec<u8> {
    let mut rgb = Vec::with_capacity(320 * 180 * 3);
    for y in 0..180_u32 {
        for x in 0..320_u32 {
            let byte = |value: u32| u8::try_from(value % 256).unwrap();
            rgb.extend_from_slice(&[byte(x), byte(y), byte(x ^ y)]);
        }
    }
    webp::Encoder::from_rgb(&rgb, 320, 180).encode(50.0).to_vec()
}

fn bench(c: &mut Criterion) {
    let picture = stored();
    let turned = ThumbView {
        before: Orientation::CLOCKWISE,
        crop: Some((
            Crop {
                top: 100,
                right: 200,
                bottom: 100,
                left: 200,
            },
            Size {
                width: 1920,
                height: 1080,
            },
        )),
        after: Orientation::MIRROR_HORIZONTAL,
    };
    c.bench_function("thumb_show_plain", |b| {
        b.iter(|| render(black_box(&picture), &ThumbView::default()).unwrap());
    });
    c.bench_function("thumb_show_cropped_turned", |b| {
        b.iter(|| render(black_box(&picture), &turned).unwrap());
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
