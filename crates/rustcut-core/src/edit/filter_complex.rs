//! The `-filter_complex` graph of a multi-section render: one input per section (accurate `-ss`/`-to` input seeking), joined
//! by `concat` (video and every audio track), then the filter chain (ADR-3).

use std::fmt::Write as _;

/// What each input (one per section) carries; all sections come from the same source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConcatInput {
    /// Number of sections, i.e. ffmpeg inputs `0..sections`.
    pub sections: usize,
    /// Audio tracks of the source; every one is kept (PyCutRv kept only the first).
    pub audio_tracks: usize,
}

/// The graph text plus the output labels to `-map`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConcatGraph {
    pub graph: String,
    /// Label of the video output, without brackets (`-map [vout]`).
    pub video: String,
    /// Labels of the audio outputs in track order (`aout0`, `aout1`, ...).
    pub audio: Vec<String>,
}

/// Builds the graph for `input` with `video_chain` (the lavfi text of the filter chain, may be empty) applied after the concat.
/// `None` when there is no section.
pub fn concat_graph(input: ConcatInput, video_chain: &str) -> Option<ConcatGraph> {
    if input.sections == 0 {
        return None;
    }
    let audio: Vec<String> = (0..input.audio_tracks).map(|t| format!("aout{t}")).collect();
    // Writing into a String cannot fail; the results are ignored.
    let mut graph = String::new();
    for i in 0..input.sections {
        let _ = write!(graph, "[{i}:v:0]");
        for t in 0..input.audio_tracks {
            let _ = write!(graph, "[{i}:a:{t}]");
        }
    }
    let _ = write!(graph, "concat=n={}:v=1:a={}[vcat]", input.sections, input.audio_tracks);
    for label in &audio {
        let _ = write!(graph, "[{label}]");
    }
    let chain = if video_chain.is_empty() { "null" } else { video_chain };
    let _ = write!(graph, ";[vcat]{chain}[vout]");
    Some(ConcatGraph {
        graph,
        video: "vout".to_owned(),
        audio,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_sections_no_graph() {
        assert_eq!(
            concat_graph(
                ConcatInput {
                    sections: 0,
                    audio_tracks: 1
                },
                ""
            ),
            None
        );
    }

    #[test]
    fn graphs_for_typical_sources() {
        let one = concat_graph(
            ConcatInput {
                sections: 1,
                audio_tracks: 1,
            },
            "",
        )
        .unwrap();
        let three_two_tracks = concat_graph(
            ConcatInput {
                sections: 3,
                audio_tracks: 2,
            },
            "hflip,scale=w=1280:h=-2",
        )
        .unwrap();
        let silent = concat_graph(
            ConcatInput {
                sections: 2,
                audio_tracks: 0,
            },
            "hflip",
        )
        .unwrap();
        assert_eq!(three_two_tracks.audio, vec!["aout0", "aout1"]);
        assert!(silent.audio.is_empty());
        insta::assert_snapshot!(format!("{}\n{}\n{}", one.graph, three_two_tracks.graph, silent.graph));
    }
}
