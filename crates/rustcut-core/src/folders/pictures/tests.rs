use std::collections::HashSet;

use image::{ImageFormat, Rgba as Px, RgbaImage};

use super::store::KEEP_NEW_SECS;
use super::*;

fn encoded(picture: &RgbaImage, format: ImageFormat) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(picture.clone())
        .write_to(&mut out, format)
        .unwrap();
    out.into_inner()
}

#[test]
fn images_are_scaled_down_to_webp_with_alpha() {
    // 600 x 300, the left half transparent.
    let picture = RgbaImage::from_fn(600, 300, |x, _| {
        if x < 300 {
            Px([0, 0, 0, 0])
        } else {
            Px([200, 40, 40, 255])
        }
    });
    let prepared = prepare(&encoded(&picture, ImageFormat::Png)).unwrap();
    assert_eq!(prepared.kind, PictureKind::WebP);
    assert_eq!(
        (prepared.width, prepared.height),
        (256, 128),
        "fits 256 px, keeps the aspect"
    );
    assert!(prepared.data.starts_with(b"RIFF"));
    let back = decode_webp(&prepared.data).unwrap();
    assert_eq!((back.width, back.height), (256, 128));
    let alpha: Vec<u8> = back.pixels.chunks(4).filter_map(|p| p.get(3).copied()).collect();
    assert!(
        alpha.iter().any(|a| *a < 10) && alpha.iter().any(|a| *a > 245),
        "alpha kept"
    );
    assert_eq!(
        prepared.hash,
        prepare(&encoded(&picture, ImageFormat::Png)).unwrap().hash,
        "stable"
    );
    assert_eq!(prepared.hash.len(), 64);
    // A small opaque JPEG keeps its size.
    let small = RgbaImage::from_pixel(40, 30, Px([10, 120, 200, 255]));
    let prepared = prepare(&encoded(&small, ImageFormat::Jpeg)).unwrap();
    assert_eq!((prepared.width, prepared.height), (40, 30));
}

#[test]
fn what_is_no_picture_is_refused() {
    assert!(matches!(prepare(b"not a picture"), Err(PictureError::Unreadable(_))));
    assert!(matches!(prepare(b"<svg"), Err(PictureError::Unreadable(_))));
    let empty = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"></svg>"#;
    assert_eq!(prepare(empty), Err(PictureError::Empty));
    let huge = vec![0_u8; usize::try_from(MAX_INPUT).unwrap() + 1];
    assert_eq!(prepare(&huge), Err(PictureError::TooLarge));
}

#[test]
fn svgs_are_sanitized_and_classified() {
    let hostile = br##"<?xml version="1.0"?>
        <svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 24 24" onload="alert(1)">
          <script>alert(2)</script>
          <image xlink:href="file:///etc/passwd" width="24" height="24"/>
          <image href="https://example.com/x.png" width="24" height="24"/>
          <a href="https://example.com"><path d="M2 2h20v20H2z" fill="#336699"/></a>
          <foreignObject><div xmlns="http://www.w3.org/1999/xhtml">x</div></foreignObject>
        </svg>"##;
    let prepared = prepare(hostile).unwrap();
    let text = String::from_utf8(prepared.data.clone()).unwrap();
    for gone in ["script", "alert", "onload", "passwd", "example.com", "foreignObject"] {
        assert!(!text.contains(gone), "{gone} in {text}");
    }
    assert_eq!(prepared.kind, PictureKind::Svg { mono: true }, "one color");
    let two = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 2 1"><rect width="1" height="1" fill="#f00"/><rect x="1" width="1" height="1" fill="#00f"/></svg>"##;
    assert_eq!(prepare(two).unwrap().kind, PictureKind::Svg { mono: false });
    let gradient = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"><defs><linearGradient id="g"><stop offset="0" stop-color="#f00"/><stop offset="1" stop-color="#00f"/></linearGradient></defs><rect width="1" height="1" fill="url(#g)"/></svg>"##;
    assert_eq!(prepare(gradient).unwrap().kind, PictureKind::Svg { mono: false });
    // A Tabler icon (strokes in currentColor) is one color and draws.
    let tabler = include_bytes!("../../../../rustcut/assets/folder-icons/folder.svg");
    assert_eq!(prepare(tabler).unwrap().kind, PictureKind::Svg { mono: true });
    let drawn = render_svg(tabler, 64).unwrap();
    assert_eq!((drawn.width, drawn.height), (64, 64));
    assert!(drawn.pixels.chunks(4).any(|p| p.get(3).is_some_and(|a| *a > 0)));
}

#[test]
fn the_store_keeps_each_picture_once_and_removes_unused_ones() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILE_NAME);
    let store = PictureStore::open(&path, &dir.path().join("backups")).unwrap();
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 4 4"><circle cx="2" cy="2" r="2"/></svg>"#;
    let a = store.import(svg).unwrap();
    let again = store.import(svg).unwrap();
    assert_eq!(a.hash, again.hash);
    let png = encoded(&RgbaImage::from_pixel(8, 8, Px([1, 2, 3, 255])), ImageFormat::Png);
    let b = store.import(&png).unwrap();
    assert_eq!(store.count().unwrap(), 2);
    assert_eq!(store.get(&a.hash).unwrap(), Some(a.clone()));
    assert_eq!(store.get("nothing").unwrap(), None);
    // New pictures stay a while even when unused (the Settings page may not have set them yet).
    let keep: HashSet<String> = std::iter::once(a.hash).collect();
    assert_eq!(store.retain(&keep).unwrap(), 0);
    let later = chrono::Utc::now().timestamp() + KEEP_NEW_SECS + 1;
    assert_eq!(store.retain_at(&keep, later).unwrap(), 1);
    assert_eq!(store.get(&b.hash).unwrap(), None);
    drop(store);
    // Opened again: still there; a newer schema is refused and left alone.
    let store = PictureStore::open(&path, &dir.path().join("backups")).unwrap();
    assert_eq!(store.count().unwrap(), 1);
    drop(store);
    rusqlite::Connection::open(&path)
        .unwrap()
        .pragma_update(None, "user_version", 99)
        .unwrap();
    assert!(matches!(
        PictureStore::open(&path, &dir.path().join("backups")),
        Err(PictureError::Store(_))
    ));
    assert!(!dir.path().join("backups").exists(), "nothing backed up or migrated");
}

#[tokio::test]
async fn the_thread_imports_files_and_answers_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("pic.png");
    std::fs::write(
        &file,
        encoded(&RgbaImage::from_pixel(300, 10, Px([9, 9, 9, 255])), ImageFormat::Png),
    )
    .unwrap();
    let (store, thread) = spawn_picture_store(PictureStore::open_in_memory().unwrap()).unwrap();
    let stored = store
        .request(|reply| PictureCommand::Import { path: file, reply })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.kind, PictureKind::WebP);
    let missing = store
        .request(|reply| PictureCommand::Import {
            path: dir.path().join("gone.png"),
            reply,
        })
        .await
        .unwrap();
    assert!(matches!(missing, Err(PictureError::Unreadable(_))));
    store.send(PictureCommand::Retain { keep: HashSet::new() }).unwrap();
    let got = store
        .request(|reply| PictureCommand::Get {
            hash: stored.hash.clone(),
            reply,
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(got, Some(stored), "a new picture survives a retain");
    store.request(|reply| PictureCommand::Shutdown { reply }).await.unwrap();
    thread.join().unwrap();
}
