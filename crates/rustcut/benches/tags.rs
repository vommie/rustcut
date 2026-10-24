//! Benchmarks of the Tag & Rate panel's tree (P6, M6.3): RustView's 5 000-tag case for the ported `TagTreeBuilder`
//! (hierarchy, flatten with everything expanded or collapsed, with a substring and a regular-expression filter, and the
//! split into columns). Run with `cargo bench -p rustcut --bench tags`.
#![allow(clippy::unwrap_used, clippy::expect_used, reason = "benchmark code")]

use std::collections::HashSet;
use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use regex::Regex;
use rustcut::TagNodeView;
use rustcut::ui::tags::{TagFlattenContext, TagTreeBuilder};
use rustcut_core::rustview::TagItem;

/// `count` tags: 50 roots, each with children, grandchildren and great-grandchildren (four levels like a real library).
fn tags(count: u32) -> Vec<TagItem> {
    let words = ["Nature", "Travel", "People", "Family", "Mountains", "Water", "City", "Night", "Winter", "Summer"];
    (1..=count)
        .map(|id| {
            let parent_id = match id {
                1..=50 => None,
                51..=500 => Some((id - 51) % 50 + 1),
                501..=2_000 => Some((id - 501) % 450 + 51),
                _ => Some((id - 2_001) % 1_500 + 501),
            };
            let word = words.get(usize::try_from(id % 10).unwrap()).unwrap();
            TagItem {
                id,
                parent_id,
                name: format!("{word} {id}"),
            }
        })
        .collect()
}

fn flatten(nodes: &[rustcut::ui::tags::TagTreeNode], expanded: &HashSet<u32>, filter: &str, regex: Option<&Regex>) -> Vec<TagNodeView> {
    let empty = HashSet::new();
    let ctx = TagFlattenContext {
        expanded_set: expanded,
        filter_text: filter,
        filter_regex: regex,
        assigned: &empty,
        locked: &empty,
        hidden: &empty,
        show_hidden: false,
    };
    let mut rows = Vec::new();
    TagTreeBuilder::flatten(nodes, 0, &ctx, &mut rows);
    rows
}

fn tag_tree(c: &mut Criterion) {
    let all = tags(5_000);
    let nodes = TagTreeBuilder::build_hierarchy(&all);
    let every: HashSet<u32> = all.iter().map(|t| t.id).collect();
    let none = HashSet::new();
    let regex = Regex::new("(?i)^(water|night) 4").unwrap();

    let mut group = c.benchmark_group("tag_tree_5000");
    group.bench_function("build_hierarchy", |b| b.iter(|| TagTreeBuilder::build_hierarchy(black_box(&all))));
    group.bench_function("flatten_collapsed", |b| b.iter(|| flatten(black_box(&nodes), &none, "", None)));
    group.bench_function("flatten_expanded", |b| b.iter(|| flatten(black_box(&nodes), &every, "", None)));
    group.bench_function("flatten_filter", |b| b.iter(|| flatten(black_box(&nodes), &none, "water 4", None)));
    group.bench_function("flatten_regex", |b| b.iter(|| flatten(black_box(&nodes), &none, "", Some(&regex))));
    let rows = flatten(&nodes, &every, "", None);
    for height in [200.0_f32, 450.0, 900.0] {
        group.bench_with_input(BenchmarkId::new("columns", height), &height, |b, h| {
            b.iter(|| TagTreeBuilder::chunk_into_columns(black_box(&rows), *h));
        });
    }
    group.finish();
}

criterion_group!(benches, tag_tree);
criterion_main!(benches);
