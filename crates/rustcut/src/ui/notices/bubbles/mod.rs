//! The bubbles' bridge (M6.5c, user decision Q-N10): keeps the [`BubbleBoard`], publishes each window's rows to its
//! `Bubbles` global (`ui/bubble.slint`; the main window and the Settings window have their own), answers the anchors'
//! `wanted` and `moved`, runs a bubble's link and its ×, and lets input checks wait for a grace time: a change that
//! comes with one waits until no other change of the same key came for that long (typing), and a later change
//! without one replaces it at once.

mod model;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::time::Duration;

use slint::{ComponentHandle, ModelRc, VecModel};

pub use self::model::{Area, BubbleButton, NewBubble, Win};
use self::model::{BubbleBoard, Place};
use super::kind::Kind;
use super::notice::{Click, Notice};
use crate::{BubbleButtonView, BubbleView, Bubbles, TipRect};

/// The usual grace time of a check while the user types (Q-N10).
pub const TYPING: Duration = Duration::from_millis(600);

/// What runs when a bubble is used: its link (or its click) and its icon buttons in order.
#[derive(Clone)]
struct Runs {
    click: Option<Click>,
    buttons: Vec<Rc<dyn Fn()>>,
}

/// A change of a bubble as it waits for its grace time: the bubble (`None` = a notice without an anchor) and what its
/// link and buttons run; `None` = the bubble goes.
type Change = Option<(Option<NewBubble>, Runs)>;

/// A window that shows bubbles.
trait BubbleWindow {
    /// Runs `f` with the window's `Bubbles` global; `false` when the window is gone.
    fn with(&self, f: &mut dyn FnMut(&Bubbles<'_>)) -> bool;
}

impl<C> BubbleWindow for slint::Weak<C>
where
    C: ComponentHandle,
    for<'a> Bubbles<'a>: slint::Global<'a, C>,
{
    fn with(&self, f: &mut dyn FnMut(&Bubbles<'_>)) -> bool {
        self.upgrade().is_some_and(|window| {
            f(&window.global::<Bubbles<'_>>());
            true
        })
    }
}

struct Slot {
    window: Box<dyn BubbleWindow>,
    model: Rc<VecModel<BubbleView>>,
}

pub struct BubbleCenter {
    me: Weak<Self>,
    board: RefCell<BubbleBoard>,
    slots: RefCell<HashMap<Win, Slot>>,
    /// What a click on the bubble of a key, and on its buttons, runs.
    clicks: RefCell<HashMap<String, Runs>>,
    /// Changes that wait for their grace time, by key.
    waiting: RefCell<HashMap<String, slint::Timer>>,
    revision: Cell<i32>,
    opacity: Cell<f32>,
    /// Each bubble's buttons as one model that lives as long as they stay the same: the layer re-publishes every
    /// epoch, and a new model would make its buttons again under a resting pointer (no hover, no tooltip).
    button_models: RefCell<ButtonModels>,
}

/// Per bubble id: its buttons and their model.
type ButtonModels = HashMap<u64, (Vec<BubbleButton>, ModelRc<BubbleButtonView>)>;

impl BubbleCenter {
    pub fn new(opacity_percent: u8) -> Rc<Self> {
        Rc::new_cyclic(|me| Self {
            me: Weak::clone(me),
            board: RefCell::default(),
            slots: RefCell::default(),
            clicks: RefCell::default(),
            waiting: RefCell::default(),
            revision: Cell::new(0),
            opacity: Cell::new(opacity(opacity_percent)),
            button_models: RefCell::default(),
        })
    }

    /// Shows the bubbles of `win` in `window` (the main window at the start, the Settings window when it is made).
    pub fn attach<C>(&self, win: Win, window: &C)
    where
        C: ComponentHandle + 'static,
        for<'a> Bubbles<'a>: slint::Global<'a, C>,
    {
        let model = Rc::new(VecModel::default());
        let bubbles = window.global::<Bubbles<'_>>();
        bubbles.set_items(ModelRc::from(Rc::clone(&model)));
        bubbles.set_opacity(self.opacity.get());
        let me = Weak::clone(&self.me);
        bubbles.on_wanted(move |key, _revision| me.upgrade().is_some_and(|c| c.board.borrow().wants(&key)));
        let me = Weak::clone(&self.me);
        bubbles.on_moved(move |key, place, epoch| {
            if let Some(center) = me.upgrade() {
                center.moved(win, &key, &place, epoch);
            }
        });
        let me = Weak::clone(&self.me);
        bubbles.on_clicked(move |id| {
            if let Some(center) = me.upgrade() {
                center.clicked(id);
            }
        });
        let me = Weak::clone(&self.me);
        bubbles.on_button_clicked(move |id, index| {
            if let Some(center) = me.upgrade() {
                center.button_clicked(id, index);
            }
        });
        let me = Weak::clone(&self.me);
        bubbles.on_closed(move |id| {
            if let Some(center) = me.upgrade() {
                center.closed(id);
            }
        });
        self.slots.borrow_mut().insert(
            win,
            Slot {
                window: Box::new(window.as_weak()),
                model,
            },
        );
        self.publish(win);
    }

    /// The anchors of `win` report again (a new epoch): a bubble whose anchor is gone stops showing before the next
    /// frame.
    pub fn look_again(&self, win: Win) {
        if self.board.borrow().rows(win, 0).is_empty() {
            return;
        }
        if let Some(slot) = self.slots.borrow().get(&win) {
            slot.window.with(&mut |b| b.set_epoch(b.get_epoch().wrapping_add(1)));
        }
    }

    /// The setting's opacity (percent) changed.
    pub fn set_opacity(&self, percent: u8) {
        self.opacity.set(opacity(percent));
        for slot in self.slots.borrow().values() {
            slot.window.with(&mut |b| b.set_opacity(self.opacity.get()));
        }
    }

    /// Shows `notice` as the bubble of `key` (`None` = its problem is gone), after `grace` when it has one.
    pub fn set(&self, key: &str, notice: Option<&Notice>, grace: Duration) {
        let change = notice.map(|n| {
            let link = n.actions.iter().find(|a| a.icon.is_none()).map(|a| Rc::clone(&a.run));
            let runs = Runs {
                click: link.map(Click::Run).or_else(|| n.click.clone()),
                buttons: n
                    .actions
                    .iter()
                    .filter(|a| a.icon.is_some())
                    .map(|a| Rc::clone(&a.run))
                    .collect(),
            };
            (to_bubble(n), runs)
        });
        if grace.is_zero() {
            self.waiting.borrow_mut().remove(key);
            self.apply(key, change);
            return;
        }
        let me = Weak::clone(&self.me);
        let owned = key.to_owned();
        let timer = slint::Timer::default();
        timer.start(slint::TimerMode::SingleShot, grace, move || {
            if let Some(center) = me.upgrade() {
                center.waiting.borrow_mut().remove(&owned);
                center.apply(&owned, change.clone());
            }
        });
        self.waiting.borrow_mut().insert(key.to_owned(), timer);
    }

    /// Drops the bubbles whose kind `keep` refuses (the user moved the kind elsewhere).
    pub fn retain(&self, keep: impl Fn(Kind) -> bool) {
        let changed = self.board.borrow_mut().retain(|b| keep(b.kind));
        self.changed(changed);
    }

    fn apply(&self, key: &str, change: Change) {
        let changed = match change {
            Some((Some(bubble), runs)) => {
                self.clicks.borrow_mut().insert(key.to_owned(), runs);
                self.board.borrow_mut().set(key, bubble)
            }
            // A notice without an anchor cannot be a bubble.
            Some((None, ..)) => return,
            None => {
                self.clicks.borrow_mut().remove(key);
                self.board.borrow_mut().clear(key)
            }
        };
        self.changed(changed);
    }

    /// Publishes every window; a new revision when the anchors that want a bubble changed.
    fn changed(&self, anchors: bool) {
        if anchors {
            self.revision.set(self.revision.get().wrapping_add(1));
            for slot in self.slots.borrow().values() {
                slot.window.with(&mut |b| b.set_revision(self.revision.get()));
            }
        }
        self.publish(Win::Main);
        self.publish(Win::Settings);
    }

    fn moved(&self, win: Win, key: &str, place: &TipRect, epoch: i32) {
        let place = Place {
            x: place.x,
            y: place.y,
            width: place.width,
            height: place.height,
        };
        let changed = self.board.borrow_mut().moved(key, place, epoch);
        if changed {
            self.publish(win);
        }
    }

    fn clicked(&self, id: i32) {
        let key = u64::try_from(id)
            .ok()
            .and_then(|id| self.board.borrow().get(id).map(|(key, _)| key.to_owned()));
        let click = key.and_then(|key| self.clicks.borrow().get(&key).and_then(|r| r.click.clone()));
        match click {
            Some(Click::Run(run)) => run(),
            Some(Click::Jump(target)) => crate::ui::jump::go(target),
            None => {}
        }
    }

    /// An icon button of the bubble `id` (its problem is solved by it, so its check clears the bubble next).
    fn button_clicked(&self, id: i32, index: i32) {
        let key = u64::try_from(id)
            .ok()
            .and_then(|id| self.board.borrow().get(id).map(|(key, _)| key.to_owned()));
        let run = key.and_then(|key| {
            let index = usize::try_from(index).ok()?;
            self.clicks
                .borrow()
                .get(&key)
                .and_then(|r| r.buttons.get(index).cloned())
        });
        if let Some(run) = run {
            run();
        }
    }

    fn closed(&self, id: i32) {
        let Ok(id) = u64::try_from(id) else { return };
        let changed = self.board.borrow_mut().hide(id);
        self.changed(changed);
    }

    fn publish(&self, win: Win) {
        let slots = self.slots.borrow();
        let Some(slot) = slots.get(&win) else { return };
        let mut epoch = 0;
        if !slot.window.with(&mut |b| epoch = b.get_epoch()) {
            return;
        }
        let board_rows = self.board.borrow().rows(win, epoch);
        let mut models = self.button_models.borrow_mut();
        let rows: Vec<BubbleView> = board_rows
            .into_iter()
            .map(|r| BubbleView {
                id: i32::try_from(r.id).unwrap_or(i32::MAX),
                area: r.area.id().into(),
                level: r.level.index(),
                title: r.title.into(),
                text: r.detail.into(),
                link: r.link.into(),
                buttons: button_model(&mut models, r.id, &r.buttons),
                anchor: TipRect {
                    x: r.place.x,
                    y: r.place.y,
                    width: r.place.width,
                    height: r.place.height,
                },
                seen: r.seen,
            })
            .collect();
        // Bubbles that are gone lose their buttons' model.
        let board = self.board.borrow();
        models.retain(|id, _| board.get(*id).is_some());
        drop(board);
        drop(models);
        crate::util::model::sync_keyed(&slot.model, rows, |r| r.id);
    }
}

/// The buttons' model of bubble `id`: the same one while they stay the same.
fn button_model(models: &mut ButtonModels, id: u64, buttons: &[BubbleButton]) -> ModelRc<BubbleButtonView> {
    if let Some((known, model)) = models.get(&id)
        && known.as_slice() == buttons
    {
        return model.clone();
    }
    let model = ModelRc::new(VecModel::from(
        buttons
            .iter()
            .map(|b| BubbleButtonView {
                icon: b.icon.to_slint(),
                tooltip: b.tooltip.as_str().into(),
            })
            .collect::<Vec<_>>(),
    ));
    models.insert(id, (buttons.to_vec(), model.clone()));
    model
}

fn opacity(percent: u8) -> f32 {
    f32::from(percent.clamp(20, 100)) / 100.0
}

fn to_bubble(notice: &Notice) -> Option<NewBubble> {
    let anchor = notice.anchor.as_ref()?;
    Some(NewBubble {
        kind: notice.kind,
        level: notice.level,
        title: notice.title.clone(),
        detail: notice.detail.clone(),
        link: notice
            .actions
            .iter()
            .find(|a| a.icon.is_none())
            .map(|a| a.label.clone())
            .unwrap_or_default(),
        buttons: notice
            .actions
            .iter()
            .filter_map(|a| {
                a.icon.map(|icon| BubbleButton {
                    icon,
                    tooltip: a.label.clone(),
                })
            })
            .collect(),
        area: anchor.area,
        anchors: anchor.keys.clone(),
    })
}
