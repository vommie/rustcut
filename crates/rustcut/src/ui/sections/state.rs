//! The sections of the open video as the editor sees them (pure, tested): the current section's start and end marks, the
//! list of added sections (the core's [`SectionList`], stable ids) with its selection of several rows (P9.5), and every
//! edit the Sections card, the timeline and the keys make (what acts on several rows is in `arrange.rs`). Times are
//! limited to the video's duration once it is known.

use rustcut_core::edit::{Section, SectionError, SectionId, SectionList};
use rustcut_core::settings::AutoSection;
use rustcut_core::time::Timestamp;

use crate::util::selection::Selection;

/// One end of a section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Bound {
    Start,
    End,
}

/// Whose edge: the current section's marks or a section of the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeOwner {
    Marks,
    Section(SectionId),
}

/// An edge that can be dragged on the timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Edge {
    pub owner: EdgeOwner,
    pub bound: Bound,
}

/// Why an edit was refused (shown in the status line).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EditError {
    #[error("Set a section start and end first")]
    NoRange,
    #[error("A section needs its end after its start")]
    Empty,
    #[error("This section is already in the list")]
    Duplicate,
    #[error("\"{0}\" is not a time (use H:MM:SS.mmm)")]
    NotATime(String),
    #[error("The section no longer exists")]
    Gone,
}

/// One row of the section list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionRow {
    pub id: SectionId,
    /// 1-based position (the output order).
    pub number: usize,
    pub start: Timestamp,
    pub end: Timestamp,
    pub selected: bool,
    /// Shares time with another section (rendered twice).
    pub overlapping: bool,
}

/// The list, the marks and the selection without the duration: the sections of a draft that waits while a job is
/// edited (user decision Q-Q3).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SectionsDraft {
    pub list: SectionList,
    pub start: Option<Timestamp>,
    pub end: Option<Timestamp>,
    pub selection: Selection<SectionId>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SectionsState {
    pub(super) list: SectionList,
    start: Option<Timestamp>,
    end: Option<Timestamp>,
    pub(super) selection: Selection<SectionId>,
    duration: Option<Timestamp>,
}

impl SectionsState {
    /// Another video was opened: its sections start empty, the start mark at 0 (as in RustView, user decision
    /// 2026-10-27).
    pub fn clear(&mut self) {
        self.list.clear();
        self.start = Some(Timestamp::ZERO);
        self.end = None;
        self.selection = Selection::default();
        self.duration = None;
    }

    /// The sections of the draft as they are.
    pub fn draft(&self) -> SectionsDraft {
        SectionsDraft {
            list: self.list.clone(),
            start: self.start,
            end: self.end,
            selection: self.selection.clone(),
        }
    }

    /// Puts a draft back after its video opened again (the duration follows and limits it).
    pub fn put_draft(&mut self, draft: SectionsDraft) {
        self.list = draft.list;
        self.start = draft.start;
        self.end = draft.end;
        self.selection = draft.selection;
        self.forget_gone();
        if let Some(d) = self.duration {
            self.set_duration(Some(d));
        }
    }

    /// The sections of a job loaded into the editor: the first one is the current section (the marks on its bounds,
    /// selected; the editor seeks to its start), a job without sections gets the start mark at 0. Returns where the
    /// first section starts.
    pub fn load(&mut self, list: SectionList) -> Option<Timestamp> {
        let first = list.iter().next().map(|(id, s)| (id, s.start(), s.end()));
        self.put_draft(SectionsDraft {
            list,
            start: Some(first.map_or(Timestamp::ZERO, |(_, start, _)| start)),
            end: first.map(|(_, _, end)| end),
            selection: Selection::default(),
        });
        let (id, start, _) = first?;
        self.select(Some(id));
        Some(start)
    }

    /// The section around `time`: one of the list, else the range between the marks.
    pub fn section_at(&self, time: Timestamp) -> Option<(Timestamp, Timestamp)> {
        let listed = self.list.sections().find(|s| s.contains(time));
        listed.map(|s| (s.start(), s.end())).or_else(|| {
            let (start, end) = (self.start?, self.end?);
            (start <= time && time < end).then_some((start, end))
        })
    }

    /// Whether the draft holds work that a replacement would lose (sections or marks; the start mark at 0 that every
    /// video gets is none).
    pub fn has_work(&self) -> bool {
        !self.list.is_empty() || self.start.is_some_and(|s| s != Timestamp::ZERO) || self.end.is_some()
    }

    /// After a save with "clear sections": the list goes, the marks stay (PyCutRv).
    pub fn clear_list(&mut self) {
        self.list.clear();
        self.selection = Selection::default();
    }

    /// The video's duration once known (times are limited to it from now on; the list is cut to it).
    pub fn set_duration(&mut self, duration: Option<Timestamp>) {
        self.duration = duration.filter(|d| d.as_micros() > 0);
        if let Some(d) = self.duration {
            self.list.clamp_to(d);
            self.forget_gone();
            self.start = self.start.map(|t| t.min(d));
            self.end = self.end.map(|t| t.min(d));
        }
    }

    pub fn duration(&self) -> Option<Timestamp> {
        self.duration
    }

    pub fn list(&self) -> &SectionList {
        &self.list
    }

    /// The current section's start and end marks.
    pub fn marks(&self) -> (Option<Timestamp>, Option<Timestamp>) {
        (self.start, self.end)
    }

    pub fn mark(&self, bound: Bound) -> Option<Timestamp> {
        match bound {
            Bound::Start => self.start,
            Bound::End => self.end,
        }
    }

    /// Sets a mark (from the playhead or a typed time), as RustView does (user decision 2026-10-27): both marks may
    /// share a time; an end without a start puts the start at 0; an end before the start moves the start onto it. A
    /// start after the end clears the end (a new section begins).
    pub fn set_mark(&mut self, bound: Bound, t: Timestamp) {
        let t = self.limit(t);
        match bound {
            Bound::Start => {
                self.start = Some(t);
                if self.end.is_some_and(|e| e < t) {
                    self.end = None;
                }
            }
            Bound::End => {
                self.end = Some(t);
                self.start = Some(self.start.map_or(Timestamp::ZERO, |s| s.min(t)));
            }
        }
    }

    /// A typed mark; empty text clears it.
    pub fn set_mark_text(&mut self, bound: Bound, text: &str) -> Result<(), EditError> {
        if text.trim().is_empty() {
            match bound {
                Bound::Start => self.start = None,
                Bound::End => self.end = None,
            }
            return Ok(());
        }
        let t = parse(text)?;
        self.set_mark(bound, t);
        Ok(())
    }

    /// The range between the marks, when both are set and in order.
    pub fn current(&self) -> Result<Section, EditError> {
        let (Some(start), Some(end)) = (self.start, self.end) else {
            return Err(EditError::NoRange);
        };
        if end <= start {
            return Err(EditError::Empty);
        }
        Section::new(start, end).map_err(|_empty| EditError::Empty)
    }

    /// The one section a save without listed sections gets (`auto`, PyCutRv's auto section), or why there is none.
    pub fn auto_section(&self, auto: AutoSection) -> Result<Section, &'static str> {
        let whole = || {
            self.duration
                .and_then(|d| Section::new(Timestamp::ZERO, d).ok())
                .ok_or("The video's length is not known yet.")
        };
        match auto {
            AutoSection::Off => Err("Add a section first (or switch on the automatic section in the settings)."),
            AutoSection::WholeVideo => whole(),
            AutoSection::CurrentRange => self
                .current()
                .map_err(|_none| "Add a section first, or set both marks for the automatic section."),
            AutoSection::RangeOrWhole => match (self.start, self.end) {
                // The start mark at 0 alone is where every video begins: the whole video.
                (None | Some(Timestamp::ZERO), None) => whole(),
                (Some(_), Some(_)) => self
                    .current()
                    .map_err(|_empty| "Add a section first, or set both marks for the automatic section."),
                _ => Err("Only one mark is set: set the other one, clear it, or add a section."),
            },
        }
    }

    /// Appends the current range to the list and selects it. The marks stay (PyCutRv).
    pub fn add_current(&mut self) -> Result<SectionId, EditError> {
        let section = self.current()?;
        if self.list.sections().any(|s| s == section) {
            return Err(EditError::Duplicate);
        }
        let id = self.list.push(section);
        self.selection.click(id);
        Ok(id)
    }

    /// The current section (the last one clicked) while it is selected.
    pub fn selected(&self) -> Option<SectionId> {
        self.selection.current().filter(|id| self.selection.contains(*id))
    }

    /// Selects only this section (unknown ids clear the selection).
    pub fn select(&mut self, id: Option<SectionId>) {
        match id.filter(|id| self.list.get(*id).is_some()) {
            Some(id) => self.selection.click(id),
            None => self.selection.clear(),
        }
    }

    /// A click on the row at `index` with the keys held (Ctrl one more or less, Shift a range; as in the queue).
    pub fn pick(&mut self, index: usize, control: bool, shift: bool) {
        let Some(id) = self.id_at(index) else { return };
        let order: Vec<SectionId> = self.list.ids().collect();
        self.selection.pick(id, &order, control, shift);
    }

    /// Whether the section is selected.
    pub fn is_selected(&self, id: SectionId) -> bool {
        self.selection.contains(id)
    }

    /// Drops ids that are no longer listed from the selection.
    pub(super) fn forget_gone(&mut self) {
        let ids: Vec<SectionId> = self.list.ids().collect();
        self.selection.retain(|id| ids.contains(&id), |id| ids.contains(&id));
    }

    /// The id of the row at `index` (0-based).
    pub fn id_at(&self, index: usize) -> Option<SectionId> {
        self.list.ids().nth(index)
    }

    /// A typed start or end of a listed section. Reversed bounds are swapped (the core's rule), equal ones refused.
    pub fn edit(&mut self, id: SectionId, bound: Bound, text: &str) -> Result<(), EditError> {
        let t = self.limit(parse(text)?);
        let section = self.list.get(id).ok_or(EditError::Gone)?;
        let edited = match bound {
            Bound::Start => section.with_start(t),
            Bound::End => section.with_end(t),
        }
        .map_err(|e| match e {
            SectionError::Empty(_) | SectionError::Negative(_) => EditError::Empty,
        })?;
        self.list.replace(id, edited);
        Ok(())
    }

    /// Where an edge is now.
    pub fn edge_time(&self, edge: Edge) -> Option<Timestamp> {
        match edge.owner {
            EdgeOwner::Marks => self.mark(edge.bound),
            EdgeOwner::Section(id) => self.list.get(id).map(|s| match edge.bound {
                Bound::Start => s.start(),
                Bound::End => s.end(),
            }),
        }
    }

    /// The other end of the edge's owner (the limit a drag cannot pass).
    pub fn opposite(&self, edge: Edge) -> Option<Timestamp> {
        let other = match edge.bound {
            Bound::Start => Bound::End,
            Bound::End => Bound::Start,
        };
        self.edge_time(Edge { bound: other, ..edge })
    }

    /// Moves an edge while it is dragged: it stops `gap` before its opposite edge and stays inside the video.
    pub fn drag_edge(&mut self, edge: Edge, t: Timestamp, gap: Timestamp) {
        let t = self.limit(match (edge.bound, self.opposite(edge)) {
            (Bound::Start, Some(end)) => t.min(end - gap),
            (Bound::End, Some(start)) => t.max(start + gap),
            _ => t,
        });
        match edge.owner {
            EdgeOwner::Marks => match edge.bound {
                Bound::Start => self.start = Some(t),
                Bound::End => self.end = Some(t),
            },
            EdgeOwner::Section(id) => {
                let Some(section) = self.list.get(id) else { return };
                let moved = match edge.bound {
                    Bound::Start => section.with_start(t),
                    Bound::End => section.with_end(t),
                };
                if let Ok(moved) = moved {
                    self.list.replace(id, moved);
                }
            }
        }
    }

    /// Every time a cut can be made at, sorted: the beginning, the end, the marks and every section bound.
    pub fn cut_points(&self) -> Vec<Timestamp> {
        let mut points: Vec<Timestamp> = [Some(Timestamp::ZERO), self.duration, self.start, self.end]
            .into_iter()
            .flatten()
            .chain(self.list.sections().flat_map(|s| [s.start(), s.end()]))
            .collect();
        points.sort_unstable();
        points.dedup();
        points
    }

    /// Whether `text` would be accepted as the mark `_bound` (nothing changes; the bubble of a refused time, M6.5c).
    pub fn check_mark_text(_bound: Bound, text: &str) -> Result<(), EditError> {
        if text.trim().is_empty() {
            return Ok(());
        }
        parse(text).map(|_| ())
    }

    /// Whether `text` would be accepted as the bound of the section `id` (nothing changes).
    pub fn check_edit(&self, id: SectionId, bound: Bound, text: &str) -> Result<(), EditError> {
        let t = self.limit(parse(text)?);
        let section = self.list.get(id).ok_or(EditError::Gone)?;
        match bound {
            Bound::Start => section.with_start(t),
            Bound::End => section.with_end(t),
        }
        .map(|_| ())
        .map_err(|e| match e {
            SectionError::Empty(_) | SectionError::Negative(_) => EditError::Empty,
        })
    }

    /// Every section that shares a part with an earlier one: its row index and the earlier section's number (the
    /// first such), in list order.
    pub fn overlaps(&self) -> Vec<(usize, usize)> {
        let ids: Vec<SectionId> = self.list.iter().map(|(id, _)| id).collect();
        let pairs = self.list.overlapping_pairs();
        ids.iter()
            .enumerate()
            .filter_map(|(later, id)| {
                pairs
                    .iter()
                    .filter(|(_, b)| b == id)
                    .filter_map(|(a, _)| ids.iter().position(|x| x == a))
                    .min()
                    .map(|earlier| (later, earlier + 1))
            })
            .collect()
    }

    /// The list for the Sections card and the timeline.
    pub fn rows(&self) -> Vec<SectionRow> {
        let overlapping: Vec<SectionId> = self
            .list
            .overlapping_pairs()
            .into_iter()
            .flat_map(|(a, b)| [a, b])
            .collect();
        self.list
            .iter()
            .enumerate()
            .map(|(i, (id, section))| SectionRow {
                id,
                number: i + 1,
                start: section.start(),
                end: section.end(),
                selected: self.selection.contains(id),
                overlapping: overlapping.contains(&id),
            })
            .collect()
    }

    fn limit(&self, t: Timestamp) -> Timestamp {
        let t = t.max(Timestamp::ZERO);
        self.duration.map_or(t, |d| t.min(d))
    }
}

fn parse(text: &str) -> Result<Timestamp, EditError> {
    Timestamp::parse(text).map_err(|_unparsable| EditError::NotATime(text.trim().to_owned()))
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
