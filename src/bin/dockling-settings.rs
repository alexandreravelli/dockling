//! Dockling settings: a small GTK 3 window that edits `~/.config/dockling/config.toml`
//! and applies every change to the running dock right away.

use dockling::config::{self, ClickAction, Color, Config, Position};
use dockling::desktop::{self, App};
use gtk::prelude::*;
use gtk::{gdk, gdk_pixbuf, glib};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::time::Duration;

const APP_ID: &str = "io.github.alexandreravelli.Dockling.Settings";

struct State {
    path: PathBuf,
    cfg: RefCell<Config>,
    apps: RefCell<Vec<App>>,
    pending: RefCell<Option<glib::SourceId>>,
    /// The pinned list was edited here since the last save.
    pinned_changed: Cell<bool>,
}

impl State {
    /// Applies a change and saves it a moment later (sliders fire many events).
    fn update(self: &Rc<Self>, change: impl FnOnce(&mut Config)) {
        change(&mut self.cfg.borrow_mut());
        if let Some(id) = self.pending.borrow_mut().take() {
            id.remove();
        }
        let state = self.clone();
        let id = glib::timeout_add_local_once(Duration::from_millis(250), move || {
            state.pending.borrow_mut().take();
            state.save_now();
        });
        *self.pending.borrow_mut() = Some(id);
    }

    fn save_now(&self) {
        // Pins may have been dragged or toggled in the dock meanwhile: keep those.
        if !self.pinned_changed.replace(false) {
            self.cfg.borrow_mut().pinned = config::load(&self.path).pinned;
        }
        if let Err(e) = config::save(&self.path, &self.cfg.borrow()) {
            eprintln!("dockling-settings: cannot save {}: {e}", self.path.display());
            return;
        }
        run_dock(&["--reload"]);
    }
}

/// Runs the `dockling` installed next to this program (or found in PATH).
fn run_dock(args: &[&str]) -> bool {
    let exe = std::env::current_exe()
        .ok()
        .and_then(|p| Some(p.parent()?.join("dockling")))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from("dockling"));
    Command::new(exe).args(args).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

fn main() -> glib::ExitCode {
    let app = gtk::Application::builder().application_id(APP_ID).build();
    app.connect_activate(build_window);
    app.run()
}

fn build_window(app: &gtk::Application) {
    if let Some(window) = app.active_window() {
        window.present();
        return;
    }
    let path = config::config_path();
    let state = Rc::new(State {
        cfg: RefCell::new(config::load(&path)),
        path,
        apps: RefCell::new(desktop::load_apps()),
        pending: RefCell::new(None),
        pinned_changed: Cell::new(false),
    });

    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Dock Settings")
        .icon_name("dockling")
        .default_width(600)
        .default_height(640)
        .build();
    let stack = gtk::Stack::builder().transition_type(gtk::StackTransitionType::Crossfade).build();
    let header = gtk::HeaderBar::builder().show_close_button(true).build();
    header.set_custom_title(Some(&gtk::StackSwitcher::builder().stack(&stack).build()));
    let reset = gtk::Button::with_label("Reset");
    reset.set_tooltip_text(Some("Restore the default settings (pinned apps are kept)"));
    header.pack_start(&reset);
    window.set_titlebar(Some(&header));
    window.set_title("Dock Settings");
    window.add(&stack);
    fill_stack(&stack, &state);

    reset.connect_clicked(glib::clone!(
        #[weak]
        window,
        #[weak]
        stack,
        #[strong]
        state,
        move |_| {
            if confirm_reset(&window) {
                let pinned = state.cfg.borrow().pinned.clone();
                *state.cfg.borrow_mut() = Config { pinned, ..Config::default() };
                state.save_now();
                for child in stack.children() {
                    stack.remove(&child);
                }
                fill_stack(&stack, &state);
                stack.show_all();
            }
        }
    ));
    // Save a change that is still waiting when the window closes.
    window.connect_delete_event(glib::clone!(
        #[strong]
        state,
        move |_, _| {
            if let Some(id) = state.pending.borrow_mut().take() {
                id.remove();
                state.save_now();
            }
            glib::Propagation::Proceed
        }
    ));
    window.show_all();
}

fn confirm_reset(window: &gtk::ApplicationWindow) -> bool {
    let dialog = gtk::MessageDialog::new(
        Some(window),
        gtk::DialogFlags::MODAL | gtk::DialogFlags::DESTROY_WITH_PARENT,
        gtk::MessageType::Question,
        gtk::ButtonsType::None,
        "Restore the default settings?",
    );
    dialog.set_secondary_text(Some("Your pinned apps are kept."));
    dialog.add_button("Cancel", gtk::ResponseType::Cancel);
    dialog.add_button("Reset", gtk::ResponseType::Accept);
    let answer = dialog.run();
    dialog.close();
    answer == gtk::ResponseType::Accept
}

fn fill_stack(stack: &gtk::Stack, state: &Rc<State>) {
    stack.add_titled(&appearance_page(state), "appearance", "Appearance");
    stack.add_titled(&behavior_page(state), "behavior", "Behavior");
    stack.add_titled(&pinned_page(state), "pinned", "Pinned Apps");
    stack.add_titled(&colors_page(state), "colors", "Colors");
}

// ------------------------------------------------------------------ pages

fn appearance_page(state: &Rc<State>) -> gtk::Widget {
    let (page, content) = page();
    let cfg = state.cfg.borrow().clone();

    let list = group(&content, "Placement");
    let positions: Vec<(&str, &str)> =
        Position::ALL.iter().map(|p| (p.as_str(), ["Left", "Right", "Bottom", "Top"][*p as usize])).collect();
    row(
        &list,
        "Position on screen",
        None,
        &combo(&positions, cfg.position.as_str(), state, |c, id| {
            c.position = Position::ALL.into_iter().find(|p| p.as_str() == id).unwrap_or(Position::Left);
        }),
    );
    row(
        &list,
        "Full length",
        Some("Span the whole screen edge, like Ubuntu's panel mode"),
        &switch(cfg.extend, state, |c, v| c.extend = v),
    );
    row(
        &list,
        "Keep windows clear of the dock",
        Some("Maximized windows stop at the dock's edge"),
        &switch(cfg.reserve_space, state, |c, v| c.reserve_space = v),
    );
    row(
        &list,
        "Hide automatically",
        Some("Touch the screen edge to show the dock again"),
        &switch(cfg.autohide, state, |c, v| c.autohide = v),
    );
    row(
        &list,
        "Hide after (ms)",
        None,
        &spin(cfg.autohide_delay_ms as f64, 0.0, 5000.0, 100.0, 0, state, |c, v| c.autohide_delay_ms = v as u64),
    );

    let list = group(&content, "Icons");
    let size = gtk::Scale::with_range(gtk::Orientation::Horizontal, 16.0, 128.0, 2.0);
    size.set_value(cfg.icon_size as f64);
    size.set_digits(0);
    size.set_value_pos(gtk::PositionType::Right);
    size.set_size_request(240, -1);
    size.connect_value_changed(glib::clone!(
        #[strong]
        state,
        move |s| state.update(|c| c.icon_size = s.value().round() as u32)
    ));
    row(&list, "Icon size", None, &size);
    row(
        &list,
        "Icon theme",
        Some("Leave empty to follow the XFCE theme"),
        &entry(&cfg.icon_theme, "Automatic", state, |c, v| c.icon_theme = v),
    );

    let list = group(&content, "Spacing");
    row(
        &list,
        "Padding around icons",
        None,
        &spin(cfg.icon_padding as f64, 0.0, 64.0, 1.0, 0, state, |c, v| c.icon_padding = v as u32),
    );
    row(
        &list,
        "Space between icons",
        None,
        &spin(cfg.spacing as f64, 0.0, 64.0, 1.0, 0, state, |c, v| c.spacing = v as u32),
    );
    row(
        &list,
        "Margin along the edges",
        None,
        &spin(cfg.margin as f64, 0.0, 64.0, 1.0, 0, state, |c, v| c.margin = v as u32),
    );
    row(
        &list,
        "Corner radius",
        Some("Highlights, and the dock itself when it is not full length"),
        &spin(cfg.corner_radius as f64, 0.0, 64.0, 1.0, 0, state, |c, v| c.corner_radius = v as f32),
    );
    page.upcast()
}

fn behavior_page(state: &Rc<State>) -> gtk::Widget {
    let (page, content) = page();
    let cfg = state.cfg.borrow().clone();

    let list = group(&content, "Clicks");
    let actions = [
        (ClickAction::MinimizeOrCycle.as_str(), "Minimize, or show the next window"),
        (ClickAction::Cycle.as_str(), "Show the next window"),
    ];
    row(
        &list,
        "Clicking the focused app",
        None,
        &combo(&actions, cfg.click_action.as_str(), state, |c, id| {
            c.click_action =
                if id == ClickAction::Cycle.as_str() { ClickAction::Cycle } else { ClickAction::MinimizeOrCycle };
        }),
    );

    let list = group(&content, "Content");
    row(
        &list,
        "Show running apps",
        Some("Also show open apps that are not pinned"),
        &switch(cfg.show_running, state, |c, v| c.show_running = v),
    );
    row(&list, "Show Applications button", None, &switch(cfg.show_apps_button, state, |c, v| c.show_apps_button = v));
    row(
        &list,
        "Put the button first",
        None,
        &switch(cfg.apps_button_at_start, state, |c, v| c.apps_button_at_start = v),
    );
    row(&list, "Button command", None, &entry(&cfg.apps_command, "xfce4-appfinder", state, |c, v| c.apps_command = v));

    let list = group(&content, "Tooltips and menus");
    row(&list, "Show app names on hover", None, &switch(cfg.tooltips, state, |c, v| c.tooltips = v));
    row(&list, "Text size", None, &spin(cfg.font_size as f64, 6.0, 72.0, 1.0, 0, state, |c, v| c.font_size = v as f32));
    row(
        &list,
        "Font file",
        Some("Path to a .ttf or .otf file; empty uses the system font"),
        &entry(&cfg.font, "Automatic", state, |c, v| c.font = v),
    );

    let list = group(&content, "Applications");
    row(
        &list,
        "Terminal",
        Some("Runs apps that need a terminal"),
        &entry(&cfg.terminal, "x-terminal-emulator", state, |c, v| c.terminal = v),
    );
    page.upcast()
}

fn colors_page(state: &Rc<State>) -> gtk::Widget {
    let (page, content) = page();
    let c = state.cfg.borrow().colors.clone();
    type Setter = fn(&mut Config, Color);
    let dock: [(&str, Color, Setter); 6] = [
        ("Background", c.background, |cfg, v| cfg.colors.background = v),
        ("Hovered icon", c.hover, |cfg, v| cfg.colors.hover = v),
        ("Focused app", c.active, |cfg, v| cfg.colors.active = v),
        ("Running indicator", c.indicator, |cfg, v| cfg.colors.indicator = v),
        ("App asking for attention", c.attention, |cfg, v| cfg.colors.attention = v),
        ("Show Applications button", c.apps_button, |cfg, v| cfg.colors.apps_button = v),
    ];
    let popups: [(&str, Color, Setter); 4] = [
        ("Text", c.text, |cfg, v| cfg.colors.text = v),
        ("Tooltip background", c.tooltip_background, |cfg, v| cfg.colors.tooltip_background = v),
        ("Menu background", c.menu_background, |cfg, v| cfg.colors.menu_background = v),
        ("Menu highlight", c.menu_hover, |cfg, v| cfg.colors.menu_hover = v),
    ];
    for (title, colors) in [("Dock", &dock[..]), ("Tooltips and menus", &popups[..])] {
        let list = group(&content, title);
        for &(label, color, set) in colors {
            row(&list, label, None, &color_button(color, state, set));
        }
    }
    let note = gtk::Label::new(Some("Transparency needs the compositor (Window Manager Tweaks → Compositor)."));
    note.style_context().add_class("dim-label");
    note.set_xalign(0.0);
    note.set_line_wrap(true);
    content.pack_start(&note, false, false, 0);
    page.upcast()
}

fn pinned_page(state: &Rc<State>) -> gtk::Widget {
    let (page, content) = page();
    let list = group(&content, "Pinned Apps");
    refresh_pinned(&list, state);

    let add = gtk::Button::with_label("Add Application…");
    add.set_halign(gtk::Align::Start);
    add.connect_clicked(glib::clone!(
        #[weak]
        list,
        #[strong]
        state,
        move |button| {
            let parent = button.toplevel().and_then(|w| w.downcast::<gtk::Window>().ok());
            if let Some(id) = choose_app(parent.as_ref(), &state) {
                state.pinned_changed.set(true);
                state.update(|c| c.pinned.push(id));
                refresh_pinned(&list, &state);
            }
        }
    ));
    content.pack_start(&add, false, false, 0);

    let tip = gtk::Label::new(Some(
        "Tip: you can also drag icons in the dock to reorder them, and pin or unpin apps from their right-click menu.",
    ));
    tip.style_context().add_class("dim-label");
    tip.set_xalign(0.0);
    tip.set_line_wrap(true);
    content.pack_start(&tip, false, false, 0);

    // Pins may change from the dock itself: show the current list when the page opens.
    page.connect_map(glib::clone!(
        #[weak]
        list,
        #[strong]
        state,
        move |_| {
            if state.pending.borrow().is_none() {
                state.cfg.borrow_mut().pinned = config::load(&state.path).pinned;
                refresh_pinned(&list, &state);
            }
        }
    ));
    page.upcast()
}

fn refresh_pinned(list: &gtk::ListBox, state: &Rc<State>) {
    for child in list.children() {
        list.remove(&child);
    }
    let pinned = state.cfg.borrow().pinned.clone();
    let count = pinned.len();
    for (i, entry) in pinned.iter().enumerate() {
        let resolved = desktop::resolve(&mut state.apps.borrow_mut(), entry);
        let apps = state.apps.borrow();
        let app = resolved.map(|a| &apps[a]);
        let (name, icon) = match app {
            Some(app) => (app.name.clone(), app.icon.clone()),
            None => (entry.clone(), "application-x-executable".to_string()),
        };
        let subtitle = if app.is_some() { entry.clone() } else { format!("{entry} · not installed") };

        let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        hbox.set_margin_start(12);
        hbox.set_margin_end(8);
        hbox.set_margin_top(6);
        hbox.set_margin_bottom(6);
        hbox.pack_start(&app_icon(&icon, 32), false, false, 0);
        hbox.pack_start(&labels(&name, Some(&subtitle)), true, true, 0);

        let buttons: [(&str, &str, bool, i64); 3] = [
            ("go-up-symbolic", "Move up", i > 0, -1),
            ("go-down-symbolic", "Move down", i + 1 < count, 1),
            ("list-remove-symbolic", "Unpin", true, 0),
        ];
        for (icon, tip, sensitive, step) in buttons {
            let button = gtk::Button::from_icon_name(Some(icon), gtk::IconSize::Button);
            button.set_relief(gtk::ReliefStyle::None);
            button.set_tooltip_text(Some(tip));
            button.set_sensitive(sensitive);
            button.set_valign(gtk::Align::Center);
            button.connect_clicked(glib::clone!(
                #[weak]
                list,
                #[strong]
                state,
                move |_| {
                    state.pinned_changed.set(true);
                    state.update(|c| {
                        if step == 0 {
                            c.pinned.remove(i);
                        } else {
                            c.pinned.swap(i, (i as i64 + step) as usize);
                        }
                    });
                    refresh_pinned(&list, &state);
                }
            ));
            hbox.pack_start(&button, false, false, 0);
        }
        let row = gtk::ListBoxRow::new();
        row.set_activatable(false);
        row.add(&hbox);
        list.add(&row);
    }
    if count == 0 {
        let empty = gtk::Label::new(Some("No pinned apps yet."));
        empty.style_context().add_class("dim-label");
        empty.set_margin_top(18);
        empty.set_margin_bottom(18);
        list.add(&empty);
    }
    list.show_all();
}

/// Asks for an installed application; returns its desktop id.
fn choose_app(parent: Option<&gtk::Window>, state: &Rc<State>) -> Option<String> {
    let dialog = gtk::Dialog::with_buttons(
        Some("Add Application"),
        parent,
        gtk::DialogFlags::MODAL | gtk::DialogFlags::DESTROY_WITH_PARENT,
        &[("Cancel", gtk::ResponseType::Cancel), ("Add", gtk::ResponseType::Accept)],
    );
    dialog.set_default_size(420, 520);
    dialog.set_default_response(gtk::ResponseType::Accept);
    let area = dialog.content_area();
    area.set_spacing(8);
    area.set_margin_start(12);
    area.set_margin_end(12);
    area.set_margin_top(12);

    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Search applications"));
    search.set_activates_default(true);
    area.pack_start(&search, false, false, 0);

    let mut apps: Vec<App> = {
        let pinned = state.cfg.borrow().pinned.clone();
        let mut all = state.apps.borrow_mut();
        let pinned_ids: Vec<usize> = pinned.iter().filter_map(|e| desktop::resolve(&mut all, e)).collect();
        all.iter()
            .enumerate()
            .filter(|(i, a)| !a.no_display && !pinned_ids.contains(i))
            .map(|(_, a)| a.clone())
            .collect()
    };
    apps.sort_by_key(|a| a.name.to_lowercase());

    let list = gtk::ListBox::new();
    list.set_activate_on_single_click(false);
    for app in &apps {
        let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        hbox.set_margin_start(8);
        hbox.set_margin_top(4);
        hbox.set_margin_bottom(4);
        hbox.pack_start(&app_icon(&app.icon, 24), false, false, 0);
        let label = gtk::Label::new(Some(&app.name));
        label.set_xalign(0.0);
        hbox.pack_start(&label, true, true, 0);
        let row = gtk::ListBoxRow::new();
        row.add(&hbox);
        // The widget name carries the desktop id; the tooltip shows it too.
        row.set_widget_name(&app.id);
        row.set_tooltip_text(Some(&app.id));
        list.add(&row);
    }
    if let Some(first) = list.row_at_index(0) {
        list.select_row(Some(&first));
    }
    let search_text = Rc::new(RefCell::new(String::new()));
    list.set_filter_func(Some(Box::new(glib::clone!(
        #[strong]
        search_text,
        move |row: &gtk::ListBoxRow| {
            let query = search_text.borrow();
            query.is_empty()
                || row.widget_name().to_lowercase().contains(query.as_str())
                || row.tooltip_text().is_some_and(|t| t.to_lowercase().contains(query.as_str()))
                || row
                    .child()
                    .and_then(|c| c.downcast::<gtk::Box>().ok())
                    .and_then(|b| b.children().into_iter().find_map(|w| w.downcast::<gtk::Label>().ok()))
                    .is_some_and(|l| l.text().to_lowercase().contains(query.as_str()))
        }
    ))));
    search.connect_search_changed(glib::clone!(
        #[weak]
        list,
        #[strong]
        search_text,
        move |s| {
            *search_text.borrow_mut() = s.text().to_lowercase();
            list.invalidate_filter();
            // Keep the selection on the first match, so Enter adds what is shown.
            let first = (0..).map_while(|i| list.row_at_index(i)).find(|r| r.is_child_visible());
            list.select_row(first.as_ref());
        }
    ));
    list.connect_row_activated(glib::clone!(
        #[weak]
        dialog,
        move |_, _| dialog.response(gtk::ResponseType::Accept)
    ));

    let scroller = gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::Never).vexpand(true).build();
    scroller.add(&list);
    let frame = gtk::Frame::new(None);
    frame.add(&scroller);
    area.pack_start(&frame, true, true, 0);
    dialog.show_all();
    search.grab_focus();

    let answer = dialog.run();
    let chosen = list.selected_row().map(|r| r.widget_name().to_string());
    dialog.close();
    chosen.filter(|_| answer == gtk::ResponseType::Accept)
}

// ------------------------------------------------------------------ building blocks

/// A scrollable page and the vertical box that receives its groups.
fn page() -> (gtk::ScrolledWindow, gtk::Box) {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 10);
    content.set_margin_start(24);
    content.set_margin_end(24);
    content.set_margin_top(18);
    content.set_margin_bottom(24);
    let scroller = gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::Never).build();
    scroller.add(&content);
    (scroller, content)
}

/// A titled, framed list of setting rows.
fn group(content: &gtk::Box, title: &str) -> gtk::ListBox {
    let label = gtk::Label::new(None);
    label.set_markup(&format!("<b>{}</b>", glib::markup_escape_text(title)));
    label.set_xalign(0.0);
    label.set_margin_top(8);
    content.pack_start(&label, false, false, 0);
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    let frame = gtk::Frame::new(None);
    frame.add(&list);
    content.pack_start(&frame, false, false, 0);
    list
}

fn labels(title: &str, subtitle: Option<&str>) -> gtk::Box {
    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 2);
    vbox.set_valign(gtk::Align::Center);
    let label = gtk::Label::new(Some(title));
    label.set_xalign(0.0);
    vbox.pack_start(&label, false, false, 0);
    if let Some(text) = subtitle {
        let sub = gtk::Label::new(None);
        sub.set_markup(&format!("<small>{}</small>", glib::markup_escape_text(text)));
        sub.set_xalign(0.0);
        sub.set_line_wrap(true);
        sub.style_context().add_class("dim-label");
        vbox.pack_start(&sub, false, false, 0);
    }
    vbox
}

fn row(list: &gtk::ListBox, title: &str, subtitle: Option<&str>, widget: &impl IsA<gtk::Widget>) {
    let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    hbox.set_margin_start(12);
    hbox.set_margin_end(12);
    hbox.set_margin_top(8);
    hbox.set_margin_bottom(8);
    hbox.pack_start(&labels(title, subtitle), true, true, 0);
    widget.set_valign(gtk::Align::Center);
    hbox.pack_end(widget, false, false, 0);
    let row = gtk::ListBoxRow::new();
    row.set_activatable(false);
    row.add(&hbox);
    list.add(&row);
}

fn switch(value: bool, state: &Rc<State>, set: impl Fn(&mut Config, bool) + 'static) -> gtk::Switch {
    let switch = gtk::Switch::new();
    switch.set_active(value);
    switch.connect_active_notify(glib::clone!(
        #[strong]
        state,
        move |s| {
            let v = s.is_active();
            state.update(|c| set(c, v));
        }
    ));
    switch
}

fn combo(
    items: &[(&str, &str)],
    current: &str,
    state: &Rc<State>,
    set: impl Fn(&mut Config, &str) + 'static,
) -> gtk::ComboBoxText {
    let combo = gtk::ComboBoxText::new();
    for (id, label) in items {
        combo.append(Some(id), label);
    }
    combo.set_active_id(Some(current));
    combo.connect_changed(glib::clone!(
        #[strong]
        state,
        move |c| {
            if let Some(id) = c.active_id() {
                state.update(|cfg| set(cfg, id.as_str()));
            }
        }
    ));
    combo
}

#[allow(clippy::too_many_arguments)]
fn spin(
    value: f64,
    min: f64,
    max: f64,
    step: f64,
    digits: u32,
    state: &Rc<State>,
    set: impl Fn(&mut Config, f64) + 'static,
) -> gtk::SpinButton {
    let spin = gtk::SpinButton::with_range(min, max, step);
    spin.set_digits(digits);
    spin.set_value(value);
    spin.connect_value_changed(glib::clone!(
        #[strong]
        state,
        move |s| {
            let v = s.value();
            state.update(|c| set(c, v));
        }
    ));
    spin
}

fn entry(value: &str, placeholder: &str, state: &Rc<State>, set: impl Fn(&mut Config, String) + 'static) -> gtk::Entry {
    let entry = gtk::Entry::new();
    entry.set_text(value);
    entry.set_placeholder_text(Some(placeholder));
    entry.set_width_chars(22);
    entry.connect_changed(glib::clone!(
        #[strong]
        state,
        move |e| {
            let v = e.text().trim().to_string();
            state.update(|c| set(c, v));
        }
    ));
    entry
}

fn color_button(color: Color, state: &Rc<State>, set: fn(&mut Config, Color)) -> gtk::ColorButton {
    let rgba =
        gdk::RGBA::new(color.r as f64 / 255.0, color.g as f64 / 255.0, color.b as f64 / 255.0, color.a as f64 / 255.0);
    let button = gtk::ColorButton::with_rgba(&rgba);
    button.set_use_alpha(true);
    button.connect_color_set(glib::clone!(
        #[strong]
        state,
        move |b| {
            let c = b.rgba();
            let byte = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            let color = Color { r: byte(c.red()), g: byte(c.green()), b: byte(c.blue()), a: byte(c.alpha()) };
            state.update(|cfg| set(cfg, color));
        }
    ));
    button
}

/// An application icon from the icon theme, or from a file path.
fn app_icon(icon: &str, size: i32) -> gtk::Image {
    if icon.starts_with('/')
        && let Ok(pixbuf) = gdk_pixbuf::Pixbuf::from_file_at_scale(icon, size, size, true)
    {
        return gtk::Image::from_pixbuf(Some(&pixbuf));
    }
    let theme = gtk::IconTheme::default();
    let name = if !icon.is_empty() && theme.as_ref().is_some_and(|t| t.has_icon(icon)) {
        icon
    } else {
        "application-x-executable"
    };
    let image = gtk::Image::from_icon_name(Some(name), gtk::IconSize::Dnd);
    image.set_pixel_size(size);
    image
}
