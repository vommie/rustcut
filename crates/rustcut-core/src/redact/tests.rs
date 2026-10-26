use std::path::Path;

use super::*;

fn redactor() -> Redactor {
    Redactor::with_identity(RedactKinds::default(), Some("alice".into()), Some("workstation".into()))
}

#[test]
fn paths_become_placeholders_that_stay_the_same_within_a_run() {
    let mut r = redactor();
    let line = "INFO [Open] Opened path=/home/alice/Videos/clip.mp4 size=1280x720";
    assert_eq!(
        r.redact(line),
        "INFO [Open] Opened path=<dir-1>/<file-1>.mp4 size=1280x720"
    );
    // The same file again keeps its numbers; another file in the same folder gets the next file number.
    assert_eq!(
        r.redact("[Render] /home/alice/Videos/clip.mp4 -> /home/alice/Videos/out.mkv"),
        "[Render] <dir-1>/<file-1>.mp4 -> <dir-1>/<file-2>.mkv"
    );
    // Not paths: rates, codec pairs, a URL, a lone folder.
    let plain = "x264/x265 at 12 MB/s, see https://example.org/docs/rustcut, cache in /tmp";
    assert_eq!(r.redact(plain), plain);
    // The user's name is replaced wherever it stands, also in a URL.
    assert_eq!(
        r.redact("https://github.com/alice/rustcut"),
        "https://github.com/<user>/rustcut"
    );
}

#[test]
fn taught_names_are_found_with_spaces_and_without_their_path() {
    let mut r = redactor();
    r.learn_path(Path::new("/media/Clips/basic_h264_720p - 05.mkv"));
    r.learn_tag("Anna Maria");
    r.learn_meta("Summer in Rome");
    let line =
        "[Export] Saved name=basic_h264_720p - 05.mkv into /media/Clips with tags Anna Maria, title=Summer in Rome";
    assert_eq!(
        r.redact(line),
        "[Export] Saved name=<file-1>.mkv into <dir-1> with tags <tag-1>, title=<meta-1>"
    );
    // The name without its extension (a job's name) gets the same number; a longer word keeps it.
    assert_eq!(r.redact("job basic_h264_720p - 05 done"), "job <file-1> done");
    assert_eq!(r.redact("Anna Marias"), "Anna Marias");
    // The whole path at once.
    assert_eq!(
        r.redact("/media/Clips/basic_h264_720p - 05.mkv"),
        "<dir-1>/<file-1>.mkv"
    );
}

#[test]
fn md5_user_and_host_are_replaced_and_kinds_can_be_switched_off() {
    let mut r = redactor();
    let line = "hash=0123456789abcdef0123456789abcdef by alice@workstation (alices)";
    assert_eq!(r.redact(line), "hash=<md5-1> by <user>@<host> (alices)");
    r.set_kinds(RedactKinds {
        md5: false,
        user: false,
        ..RedactKinds::default()
    });
    assert_eq!(r.redact(line), line);
    let mut off = Redactor::with_identity(
        RedactKinds {
            paths: false,
            ..RedactKinds::default()
        },
        None,
        None,
    );
    off.learn_path(Path::new("/media/Clips/clip.mp4"));
    assert_eq!(off.redact("/media/Clips/clip.mp4"), "/media/Clips/clip.mp4");
    // Too short to be taught: it would hit ordinary words.
    off.learn_tag("Art");
    assert_eq!(off.redact("Art"), "Art");
}

#[test]
fn a_real_log_line_keeps_its_shape() {
    let mut r = redactor();
    r.learn_tag("file");
    let line = "2026-10-26T02:49:50Z  INFO [Filters] Border color picked at x=871 y=669 file=/home/alice/a b/c.mkv";
    let out = r.redact(line);
    assert!(out.starts_with("2026-10-26T02:49:50Z  INFO [Filters] Border color picked at x=871 y=669 file="));
    assert!(!out.contains("alice"), "{out}");
    assert!(
        !out.contains("<<"),
        "a tag named like a placeholder word stays outside it: {out}"
    );
}
