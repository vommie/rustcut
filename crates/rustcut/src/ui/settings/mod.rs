//! The Settings window (M4.2, HANDOFF section 8.S), styled like RustView's: a sidebar of pages, cards of options, and
//! the editors of the named target folders, the render profiles and the keymap. Every change goes through
//! `app::SettingsWriter::update` at once (D-S1): there is no Save button and no unsaved state, and the other bridges
//! apply what they can while RustCut runs (`app/live.rs`); a row whose value applies at the next start says so.
//!
//! The generic options are data (`catalog/`, `field.rs`); `view.rs` turns them into rows; this bridge owns the window
//! (created at the first open, hidden on close), its models and the page state kept in `ui-state.toml`.

mod catalog;
pub mod diagnostics;
mod field;
mod folder_tree;
pub mod folders;
mod handlers;
mod hotkeys;
mod input;
mod messages;
pub mod page;
mod profiles;
mod ratios;
mod status;
mod view;

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::{Rc, Weak};

use rustcut_core::app_identity::{APP_NAME, VERSION};
use rustcut_core::settings::{AppPaths, Settings, SettingsWindowState};
use slint::{ComponentHandle as _, ModelRc, SharedString, VecModel};

use self::hotkeys::HotkeysPage;
use self::page::{Page, Special};
use self::profiles::ProfilesPage;
use crate::app::SettingsWriter;
use crate::app::window::to_dimension;
use crate::ui::folders::pictures::Pictures;
use crate::ui::folders::presence::FolderPresence;
use crate::ui::keymap::Keymap;
use crate::ui::notices::{self, Kind, Notice};
use crate::ui::render_settings::state::Encoders;
use crate::util::model::{sync, sync_with};
/// The jump mark on the selected profile's row of Settings > Render Profiles (`settings_profiles.slint`).
const PROFILE_JUMP_KEY: &str = "settings:profiles.selected";

use crate::{
    HoldConfirm, HotkeyChipView, HotkeyRowView, MainWindow, MessageKindView, OptionRowView, ProfileItemView,
    RatioChipView, RenderIssueView, SettingCardView, SettingRowView, SettingsUi, SettingsWindow, TooltipSettings,
};

/// The help text of the option `key` (`config.toml`'s name, as `Field::key`); empty for an unknown key. The filters'
/// options menus show the same texts as Settings > Filters (P9.13).
pub fn option_help(key: &str) -> &'static str {
    catalog::field(key).map_or("", |f| f.help)
}

/// What the window asks of other parts of the app (set by the app after install).
#[derive(Default)]
pub struct Hooks {
    /// What the capability check knows about the encoders (the profile editor offers those that work).
    pub encoders: Option<Box<dyn Fn() -> Encoders>>,
    /// What the app knows for the Diagnostics page (the capability check, RustView's connection, the queue's database).
    pub diagnostics: Option<Box<dyn Fn() -> rustcut_core::diagnostics::Facts>>,
    /// The RustView link's state (the test row's symbol before a test).
    pub rustview_connection: Option<Box<dyn Fn() -> rustcut_core::rustview::ConnectionState>>,
    /// The connection test: RustView is asked for its tag tree now; the answer comes on the receiver.
    pub test_rustview: Option<Box<dyn Fn() -> Option<RustViewTest>>>,
}

/// The answer of the connection test.
pub type RustViewTest = tokio::sync::oneshot::Receiver<
    Result<std::sync::Arc<rustcut_core::rustview::TagTree>, rustcut_core::rustview::RustViewError>,
>;

/// A card's row model and the rows it shows.
type CardSlot = (Rc<VecModel<SettingRowView>>, Vec<view::RowView>);

struct Models {
    pages: Rc<VecModel<SharedString>>,
    cards: Rc<VecModel<SettingCardView>>,
    /// One row model per card slot, so a card keeps its elements while its rows change, and the rows it shows.
    card_rows: RefCell<Vec<CardSlot>>,
    ratios: Rc<VecModel<RatioChipView>>,
    profiles: Rc<VecModel<ProfileItemView>>,
    issues: Rc<VecModel<RenderIssueView>>,
    video: Rc<VecModel<OptionRowView>>,
    audio: Rc<VecModel<OptionRowView>>,
    expert: Rc<VecModel<OptionRowView>>,
    hotkeys: Rc<VecModel<HotkeyRowView>>,
    kinds: Rc<VecModel<MessageKindView>>,
    muted: Rc<VecModel<MessageKindView>>,
}

pub struct SettingsBridge {
    me: Weak<Self>,
    window: RefCell<Option<SettingsWindow>>,
    writer: SettingsWriter,
    runtime: tokio::runtime::Handle,
    paths: AppPaths,
    state: RefCell<SettingsWindowState>,
    page: Cell<Page>,
    /// Why a typed text was refused, by option key (shown until a value of that option is kept).
    errors: RefCell<BTreeMap<String, String>>,
    profiles: RefCell<ProfilesPage>,
    hotkeys: RefCell<HotkeysPage>,
    /// The keymap as stored, edited by the Hotkeys page.
    keymap: RefCell<Keymap>,
    /// What the last key press on the Hotkeys page did, and whether it is a refusal.
    hotkey_note: RefCell<(String, bool)>,
    /// The Hotkeys page's rows as last shown.
    hotkeys_shown: RefCell<Vec<hotkeys::HotkeyRow>>,
    hooks: RefCell<Hooks>,
    models: Models,
    /// The Messages page: the kind tested last and what the test showed (for a few seconds), and its rows as shown.
    tested: RefCell<Option<(Kind, String)>>,
    tested_timer: slint::Timer,
    messages_shown: RefCell<messages::Shown>,
    /// The Target Folders page (P11.3) and the folder pictures it shows.
    folder_page: folder_tree::FolderPage,
    pictures: Rc<Pictures>,
    /// Which target folders are missing (checked on the runtime, P11.1).
    presence: Rc<FolderPresence>,
    /// The Diagnostics page's last report and whether its checks run (P9.16).
    diag: RefCell<diagnostics::DiagState>,
    /// The programs' and the RustView test's states beside their rows (0.0.67).
    status: RefCell<status::Board>,
}

impl SettingsBridge {
    /// Connects File > Settings, Help > About and the Help menu's folders. The window itself is made at the first open.
    pub fn install(
        main: &MainWindow,
        runtime: tokio::runtime::Handle,
        writer: SettingsWriter,
        paths: AppPaths,
        state: &SettingsWindowState,
        pictures: &Rc<Pictures>,
    ) -> Rc<Self> {
        let settings = writer.current();
        let page = Page::from_id(&state.page).unwrap_or(Page::General);
        let bridge = Rc::new_cyclic(|me| Self {
            me: Weak::clone(me),
            window: RefCell::new(None),
            writer: writer.clone(),
            runtime,
            paths,
            state: RefCell::new(state.clone()),
            page: Cell::new(page),
            errors: RefCell::default(),
            profiles: RefCell::new(ProfilesPage::new(
                settings.render.profiles.clone(),
                state.profile.as_deref(),
                &settings.render.default_profile,
            )),
            hotkeys: RefCell::new(HotkeysPage::new(state.hotkey_search.clone())),
            keymap: RefCell::new(Keymap::from_table(&settings.keymap).0),
            hotkey_note: RefCell::default(),
            hotkeys_shown: RefCell::default(),
            hooks: RefCell::default(),
            models: Models {
                pages: Rc::new(VecModel::from(
                    Page::ALL
                        .iter()
                        .map(|p| SharedString::from(p.title()))
                        .collect::<Vec<_>>(),
                )),
                cards: Rc::new(VecModel::default()),
                card_rows: RefCell::default(),
                ratios: Rc::new(VecModel::default()),
                profiles: Rc::new(VecModel::default()),
                issues: Rc::new(VecModel::default()),
                video: Rc::new(VecModel::default()),
                audio: Rc::new(VecModel::default()),
                expert: Rc::new(VecModel::default()),
                hotkeys: Rc::new(VecModel::default()),
                kinds: Rc::new(VecModel::default()),
                muted: Rc::new(VecModel::default()),
            },
            tested: RefCell::new(None),
            tested_timer: slint::Timer::default(),
            messages_shown: RefCell::default(),
            diag: RefCell::default(),
            status: RefCell::default(),
            folder_page: folder_tree::FolderPage::new(state),
            pictures: Rc::clone(pictures),
            presence: Rc::default(),
        });
        let this = Rc::downgrade(&bridge);
        main.on_open_settings(move || {
            if let Some(b) = this.upgrade() {
                b.open(None);
            }
        });
        let this = Rc::downgrade(&bridge);
        main.on_open_settings_page(move |id| {
            if let Some(b) = this.upgrade() {
                b.open(Page::from_id(&id));
            }
        });
        let this = Rc::downgrade(&bridge);
        main.on_open_folder(move |which| {
            if let Some(b) = this.upgrade() {
                b.open_folder(&which);
            }
        });
        let this = Rc::downgrade(&bridge);
        pictures.listen(move || {
            if let Some(b) = this.upgrade()
                && b.page.get().special() == Special::Folders
            {
                b.publish_folder_tree(&b.writer.current());
            }
        });
        let this = Rc::downgrade(&bridge);
        writer.subscribe(move |before, after| {
            if let Some(b) = this.upgrade() {
                b.settings_changed(before, after);
            }
        });
        bridge
    }

    pub fn hooks(&self) -> std::cell::RefMut<'_, Hooks> {
        self.hooks.borrow_mut()
    }

    /// Shows the window on `page` (`None` = the page it showed last).
    pub fn open(&self, page: Option<Page>) {
        if self.window.borrow().is_none() {
            match self.create() {
                Ok(window) => *self.window.borrow_mut() = Some(window),
                Err(e) => {
                    tracing::error!(error = %e, "[Settings] The settings window could not be opened");
                    notices::post(
                        Notice::new(Kind::SettingsWindowFailed, "The settings window could not be opened")
                            .detail(e.to_string()),
                    );
                    return;
                }
            }
        }
        if let Some(page) = page {
            self.page.set(page);
        }
        if !self.window.borrow().as_ref().is_some_and(|w| w.window().is_visible()) {
            self.status_refresh();
        }
        if let Some(encoders) = self.hooks.borrow().encoders.as_ref().map(|f| f()) {
            self.profiles.borrow_mut().set_encoders(encoders);
        }
        self.publish();
        let window = self.window.borrow();
        let Some(window) = window.as_ref() else { return };
        self.restore_position(window);
        if let Err(e) = window.show() {
            tracing::error!(error = %e, "[Settings] The settings window could not be shown");
        }
        tracing::debug!(page = self.page.get().id(), "[Settings] Window shown");
    }

    /// Shows the window on `page` and lets the option with the element key `key` flash (a jump mark, Q-N6).
    pub fn show_option(&self, page: Page, key: &str) {
        self.open(Some(page));
        if let Some(window) = self.window.borrow().as_ref() {
            crate::ui::jump::flash(&window.global::<crate::Jump<'_>>(), key);
        }
    }

    /// Shows render profile `id` in Settings > Render Profiles: selected, its row flashes (the Job panel's profile
    /// row, P9.11).
    pub fn show_profile(&self, id: &str) {
        self.open(Some(Page::Profiles));
        let index = self.profiles.borrow().index_of(id);
        if let Some(index) = index {
            self.select_profile(index);
        }
        if let Some(window) = self.window.borrow().as_ref() {
            crate::ui::jump::flash(&window.global::<crate::Jump<'_>>(), PROFILE_JUMP_KEY);
        }
    }

    /// Hides the window and remembers its size and position.
    pub fn close(&self) {
        let window = self.window.borrow();
        let Some(window) = window.as_ref() else { return };
        self.closing(window);
        if let Err(e) = window.hide() {
            tracing::warn!(error = %e, "[Settings] The settings window could not be hidden");
        }
    }

    /// The window goes away: a key being recorded is dropped, an open color picker closes, the size and position are
    /// kept.
    fn closing(&self, window: &SettingsWindow) {
        self.hotkeys.borrow_mut().cancel();
        crate::ui::color_picker::close();
        self.capture_size(window);
        self.publish();
    }

    /// The window state for `ui-state.toml` (at quit; the window closes with the app).
    pub fn save_into(&self, state: &mut SettingsWindowState) {
        if let Some(window) = self.window.borrow().as_ref()
            && window.window().is_visible()
        {
            self.capture_size(window);
        }
        let mut current = self.state.borrow().clone();
        self.page.get().id().clone_into(&mut current.page);
        current.profile = Some(self.profiles.borrow().selected_id().to_owned());
        current.hotkey_search.clone_from(&self.hotkeys.borrow().search);
        self.folder_page.save_into(&mut current);
        *state = current;
    }

    /// Hides the window at quit (the event loop has ended).
    pub fn shut_down(&self) {
        crate::ui::color_picker::shutdown();
        if let Some(window) = self.window.borrow_mut().take() {
            let _ = window.hide();
        }
    }

    /// The window's size and position while it is shown (a maximized or minimized window keeps its last normal ones).
    fn capture_size(&self, window: &SettingsWindow) {
        let w = window.window();
        if w.is_maximized() || w.is_minimized() {
            return;
        }
        let size = w.size().to_logical(w.scale_factor());
        let position = w.position();
        let mut state = self.state.borrow_mut();
        state.width = to_dimension(size.width, state.width);
        state.height = to_dimension(size.height, state.height);
        state.x = Some(position.x);
        state.y = Some(position.y);
    }

    /// Puts a hidden window back where it was last (the window manager may still place it elsewhere).
    fn restore_position(&self, window: &SettingsWindow) {
        if window.window().is_visible() {
            return;
        }
        let state = self.state.borrow();
        if let (Some(x), Some(y)) = (state.x, state.y) {
            window.window().set_position(slint::PhysicalPosition::new(x, y));
        }
    }

    fn create(&self) -> Result<SettingsWindow, slint::PlatformError> {
        let window = SettingsWindow::new()?;
        let ui = window.global::<SettingsUi<'_>>();
        let m = &self.models;
        ui.set_pages(ModelRc::from(Rc::clone(&m.pages)));
        // Each page starts at its top the first time it shows; the window keeps the positions while it is hidden.
        ui.set_page_scroll(ModelRc::from(Rc::new(VecModel::from(vec![0.0_f32; Page::ALL.len()]))));
        ui.set_cards(ModelRc::from(Rc::clone(&m.cards)));
        ui.set_ratios(ModelRc::from(Rc::clone(&m.ratios)));
        ui.set_profiles(ModelRc::from(Rc::clone(&m.profiles)));
        ui.set_profile_issues(ModelRc::from(Rc::clone(&m.issues)));
        ui.set_profile_video(ModelRc::from(Rc::clone(&m.video)));
        ui.set_profile_audio(ModelRc::from(Rc::clone(&m.audio)));
        ui.set_profile_expert(ModelRc::from(Rc::clone(&m.expert)));
        ui.set_hotkeys(ModelRc::from(Rc::clone(&m.hotkeys)));
        ui.set_hotkey_search(self.hotkeys.borrow().search.as_str().into());
        ui.set_kinds(ModelRc::from(Rc::clone(&m.kinds)));
        ui.set_muted(ModelRc::from(Rc::clone(&m.muted)));
        ui.set_app_name(APP_NAME.into());
        ui.set_app_version(VERSION.into());
        ui.set_about_paths(self.about_paths().into());
        if !self.writer.writable() {
            ui.set_read_only_note(
                "config.toml is from a newer RustCut or could not be read: changes hold until RustCut quits and are \
                 not saved."
                    .into(),
            );
        }
        input::connect(&self.me, &window);
        self.folder_page.attach(&window);
        folder_tree::connect(&self.me, &window);
        ratios::connect(&self.me, &window);
        diagnostics::connect(&self.me, &window);
        let this = Weak::clone(&self.me);
        let weak = window.as_weak();
        window.window().on_close_requested(move || {
            if let (Some(b), Some(w)) = (this.upgrade(), weak.upgrade()) {
                b.closing(&w);
            }
            slint::CloseRequestResponse::HideWindow
        });
        let state = self.state.borrow();
        window.window().set_size(slint::LogicalSize::new(
            f32::from(u16::try_from(state.width).unwrap_or(900)),
            f32::from(u16::try_from(state.height).unwrap_or(660)),
        ));
        Self::apply_look(&window, &self.writer.current());
        crate::ui::tooltip::install(&window);
        crate::ui::controls::install(&window);
        crate::ui::drag::install(&window);
        crate::ui::color_picker::install(&window);
        notices::attach_settings(&window);
        Ok(window)
    }

    /// The hold time and the tooltips of this window (it has its own globals).
    fn apply_look(window: &SettingsWindow, settings: &Settings) {
        window
            .global::<HoldConfirm<'_>>()
            .set_duration(i64::from(settings.general.hold_to_confirm_ms));
        let tooltips = window.global::<TooltipSettings<'_>>();
        tooltips.set_enabled(settings.appearance.tooltips);
        tooltips.set_delay(i64::from(settings.appearance.tooltip_delay_ms));
    }

    fn about_paths(&self) -> String {
        let p = &self.paths;
        format!(
            "Settings: {}\nData: {}\nLogs: {}\nCache: {}",
            p.config_dir.display(),
            p.data_dir.display(),
            p.log_dir().display(),
            p.cache_dir.display()
        )
    }

    /// The settings changed (here or elsewhere): the pages that mirror other parts follow, the window shows it.
    fn settings_changed(&self, before: &Settings, after: &Settings) {
        if before.render.profiles != after.render.profiles {
            self.profiles
                .borrow_mut()
                .follow(&after.render.profiles, &after.render.default_profile);
        }
        if before.keymap != after.keymap {
            *self.keymap.borrow_mut() = Keymap::from_table(&after.keymap).0;
        }
        if let Some(window) = self.window.borrow().as_ref() {
            Self::apply_look(window, after);
        }
        if before.folder_wheel != after.folder_wheel && self.page.get() == Page::Folders {
            self.play_scene();
        }
        self.publish();
    }

    /// Runs `change` on the settings through the writer (saved soon, applied by the other bridges).
    fn update(&self, change: impl FnOnce(&mut Settings)) {
        self.writer.update(change);
    }

    fn open_folder(&self, which: &str) {
        let path = match which {
            "config" => self.paths.config_dir.clone(),
            "data" => self.paths.data_dir.clone(),
            "logs" => self.paths.log_dir(),
            "cache" => self.paths.cache_dir.clone(),
            _ => return,
        };
        self.runtime.spawn_blocking(move || {
            if let Err(e) =
                std::fs::create_dir_all(&path).and_then(|()| opener::open(&path).map_err(std::io::Error::other))
            {
                tracing::warn!(folder = %path.display(), error = %e, "[Settings] The folder could not be opened");
            }
        });
    }

    /// Shows the current page.
    fn publish(&self) {
        let window = self.window.borrow();
        let Some(window) = window.as_ref() else { return };
        let ui = window.global::<SettingsUi<'_>>();
        let page = self.page.get();
        let settings = self.writer.current();
        ui.set_page(i32::try_from(page.index()).unwrap_or(0));
        ui.set_page_title(page.title().into());
        ui.set_special(match page.special() {
            Special::None => 0,
            Special::Folders => 1,
            Special::Profiles => 2,
            Special::Hotkeys => 3,
            Special::About => 4,
            Special::Messages => 5,
            Special::CropRatios => 6,
            Special::Diagnostics => 7,
        });
        ui.set_resettable(!catalog::cards(page).is_empty());
        self.check_programs(page, &settings);
        let cards = view::cards(page, &settings, &self.errors.borrow(), self);
        self.publish_cards(&cards);
        match page.special() {
            Special::Folders => self.publish_folder_tree(&settings),
            Special::Profiles => self.publish_profiles(&ui, &settings),
            Special::Hotkeys => self.publish_hotkeys(&ui),
            Special::Messages => self.publish_messages(&ui, &settings),
            Special::CropRatios => self.publish_ratios(&settings),
            Special::Diagnostics => self.publish_diagnostics(),
            Special::None | Special::About => {}
        }
    }

    fn publish_cards(&self, cards: &[view::CardView]) {
        let mut slots = self.models.card_rows.borrow_mut();
        while slots.len() < cards.len() {
            slots.push((Rc::new(VecModel::default()), Vec::new()));
        }
        let views = cards
            .iter()
            .zip(slots.iter_mut())
            .map(|(card, (rows, shown))| {
                sync_with(rows, shown, &card.rows, row_view);
                SettingCardView {
                    title: card.title.into(),
                    description: card.description.as_str().into(),
                    rows: ModelRc::from(Rc::clone(rows)),
                }
            })
            .collect();
        sync(&self.models.cards, views);
    }
}

impl view::Context for SettingsBridge {
    fn button_text(&self, key: &str) -> Option<String> {
        let p = &self.paths;
        let path = match key {
            "open.logs" => p.log_dir(),
            "open.config" => p.config_dir.clone(),
            "open.data" => p.data_dir.clone(),
            "open.cache" => p.cache_dir.clone(),
            _ => return None,
        };
        Some(path.display().to_string())
    }

    fn status(&self, key: &str) -> status::RowStatus {
        self.status.borrow().get(key)
    }
}

fn row_view(r: &view::RowView) -> SettingRowView {
    SettingRowView {
        key: r.key.into(),
        kind: r.kind as i32,
        label: r.label.into(),
        description: r.description.as_str().into(),
        checked: r.checked,
        items: ModelRc::new(VecModel::from(
            r.items.iter().map(|i| SharedString::from(*i)).collect::<Vec<_>>(),
        )),
        index: i32::try_from(r.index).unwrap_or(0),
        text: r.text.as_str().into(),
        placeholder: r.placeholder.into(),
        suffix: r.suffix.into(),
        segmented: r.kind == view::RowKind::Choice && crate::util::choice::segmented(&r.items),
        enabled: r.enabled,
        indent: r.indent,
        error: r.error.as_str().into(),
        icon: r.icon.map_or(crate::IconId::None, crate::icons::Icon::to_slint),
        status: r.status.index(),
        status_text: r.status.text().into(),
    }
}

fn hotkey_view(row: &hotkeys::HotkeyRow) -> HotkeyRowView {
    HotkeyRowView {
        id: row.action.id().into(),
        title: row.action.title().into(),
        group: hotkeys::group(row.action).into(),
        keys: ModelRc::new(VecModel::from(
            row.keys
                .iter()
                .map(|(text, label)| HotkeyChipView {
                    text: text.as_str().into(),
                    label: label.as_str().into(),
                })
                .collect::<Vec<_>>(),
        )),
        changed: row.changed,
        defaults: row.defaults.as_str().into(),
    }
}
