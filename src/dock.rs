//! The dock window: X11/EWMH plumbing, layout, drawing and input.

use crate::config::{self, ClickAction, Color, Config, Position};
use crate::desktop::{self, App};
use crate::icons::{self, IconFinder};
use crate::text::Text;
use std::borrow::Cow;
use std::collections::HashMap;
use std::error::Error;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, PixmapPaint, Rect, Transform};
use x11rb::connection::{Connection, RequestConnection};
use x11rb::image::{BitsPerPixel, Image, ImageOrder, ScanlinePad};
use x11rb::protocol::Event;
use x11rb::protocol::randr::{self, ConnectionExt as _};
use x11rb::protocol::xproto::*;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::{CURRENT_TIME, NONE};

pub type Res<T> = Result<T, Box<dyn Error>>;

pub const CMD_RELOAD: u32 = 1;
pub const CMD_QUIT: u32 = 2;

/// Thickness of the dock while auto-hidden: just enough to catch the pointer.
const HIDDEN_THICKNESS: u32 = 2;
const MAX_DOTS: usize = 4;

x11rb::atom_manager! {
    pub Atoms: AtomsCookie {
        UTF8_STRING,
        WM_CHANGE_STATE,
        _NET_CLIENT_LIST,
        _NET_CLIENT_LIST_STACKING,
        _NET_ACTIVE_WINDOW,
        _NET_WORKAREA,
        _NET_CURRENT_DESKTOP,
        _NET_CLOSE_WINDOW,
        _NET_WM_NAME,
        _NET_WM_ICON,
        _NET_WM_PID,
        _NET_WM_DESKTOP,
        _NET_WM_STRUT,
        _NET_WM_STRUT_PARTIAL,
        _NET_WM_WINDOW_TYPE,
        _NET_WM_WINDOW_TYPE_DOCK,
        _NET_WM_WINDOW_TYPE_NORMAL,
        _NET_WM_WINDOW_TYPE_DIALOG,
        _NET_WM_WINDOW_TYPE_TOOLTIP,
        _NET_WM_WINDOW_TYPE_POPUP_MENU,
        _NET_WM_STATE,
        _NET_WM_STATE_SKIP_TASKBAR,
        _NET_WM_STATE_SKIP_PAGER,
        _NET_WM_STATE_STICKY,
        _NET_WM_STATE_ABOVE,
        _NET_WM_STATE_DEMANDS_ATTENTION,
        _DOCKLING_COMMAND,
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
struct R {
    x: i32,
    y: i32,
    w: u32,
    h: u32,
}

impl R {
    fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w as i32 && y < self.y + self.h as i32
    }
}

/// A managed window shown in the dock.
struct Client {
    group: String,
    app: Option<usize>,
    name: String,
    icon_hint: String,
    attention: bool,
}

#[derive(PartialEq, Clone, Copy)]
enum Kind {
    App,
    AppsButton,
}

struct Item {
    kind: Kind,
    /// Desktop id for applications, "class:<wm class>" for unknown windows.
    key: String,
    app: Option<usize>,
    name: String,
    icon_hint: String,
    pinned: bool,
    windows: Vec<Window>,
    rect: R,
}

#[derive(Clone)]
enum Action {
    Launch(String),
    TogglePin(String),
    CloseAll(String),
    OpenSettings,
    Reload,
}

struct Menu {
    rect: R,
    entries: Vec<(String, Action)>,
    hover: Option<usize>,
    row_h: u32,
}

const MENU_PAD: u32 = 4;

/// Pointer travel (pixels) before a press on a pinned icon turns into a drag.
const DRAG_THRESHOLD: i32 = 6;

/// A pinned icon being dragged to a new position.
struct Drag {
    key: String,
    origin: (i32, i32),
    pos: (i32, i32),
    active: bool,
}

pub struct Dock {
    conn: RustConnection,
    screen_num: usize,
    root: Window,
    atoms: Atoms,
    selection: Atom,
    depth: u8,
    visual: Visualid,
    colormap: Colormap,
    argb: bool,
    win: Window,
    gc: Gcontext,
    tooltip_win: Option<Window>,
    menu_win: Option<Window>,
    tooltip_for: Option<String>,
    menu: Option<Menu>,

    cfg: Config,
    cfg_path: PathBuf,
    apps: Vec<App>,
    icons: IconFinder,
    icon_cache: HashMap<String, Option<Pixmap>>,
    font: Option<Option<Text>>,

    clients: HashMap<Window, Option<Client>>,
    order: Vec<Window>,
    active: Window,
    items: Vec<Item>,

    monitor: R,
    geom: R,
    struts: [u32; 12],
    icon_px: u32,
    hover: Option<String>,
    pressed: Option<(String, u8)>,
    drag: Option<Drag>,
    hidden: bool,
    hide_at: Option<Instant>,
    children: Vec<Child>,
}

fn intern(conn: &RustConnection, name: &str) -> Res<Atom> {
    Ok(conn.intern_atom(false, name.as_bytes())?.reply()?.atom)
}

fn selection_atom(conn: &RustConnection, screen: usize) -> Res<Atom> {
    intern(conn, &format!("_DOCKLING_S{screen}"))
}

/// Sends a command (reload/quit) to the running dock.
pub fn send_command(cmd: u32) -> Res<()> {
    let (conn, screen) = x11rb::connect(None)?;
    let owner = conn.get_selection_owner(selection_atom(&conn, screen)?)?.reply()?.owner;
    if owner == NONE {
        return Err("Dockling is not running".into());
    }
    let atom = intern(&conn, "_DOCKLING_COMMAND")?;
    let ev = ClientMessageEvent::new(32, owner, atom, [cmd, 0, 0, 0, 0]);
    conn.send_event(false, owner, EventMask::NO_EVENT, ev)?;
    // Round-trip so the server has processed the event before we disconnect.
    conn.get_input_focus()?.reply()?;
    Ok(())
}

/// The configured icon theme, else the desktop's (XSETTINGS, then XFCE's saved settings).
fn icon_theme(conn: &RustConnection, screen: usize, cfg: &Config) -> String {
    if !cfg.icon_theme.is_empty() {
        return cfg.icon_theme.clone();
    }
    let from_xsettings = || -> Option<String> {
        let owner =
            conn.get_selection_owner(intern(conn, &format!("_XSETTINGS_S{screen}")).ok()?).ok()?.reply().ok()?.owner;
        if owner == NONE {
            return None;
        }
        let prop = intern(conn, "_XSETTINGS_SETTINGS").ok()?;
        let reply = conn.get_property(false, owner, prop, prop, 0, u32::MAX / 4).ok()?.reply().ok()?;
        icons::xsettings_icon_theme(&reply.value)
    };
    from_xsettings().or_else(icons::xfconf_icon_theme).unwrap_or_default()
}

fn argb_visual(screen: &Screen) -> Option<Visualid> {
    screen
        .allowed_depths
        .iter()
        .filter(|d| d.depth == 32)
        .flat_map(|d| d.visuals.iter())
        .find(|v| v.class == VisualClass::TRUE_COLOR)
        .map(|v| v.visual_id)
}

pub fn run(replace: bool) -> Res<()> {
    let (conn, screen_num) = x11rb::connect(None)?;
    let screen = conn.setup().roots[screen_num].clone();
    let root = screen.root;
    let atoms = Atoms::new(&conn)?.reply()?;
    let selection = selection_atom(&conn, screen_num)?;
    if conn.get_selection_owner(selection)?.reply()?.owner != NONE && !replace {
        return Err("Dockling is already running (use --replace to replace it)".into());
    }

    // Use a 32-bit visual for real transparency when a compositor is running.
    let cm = intern(&conn, &format!("_NET_WM_CM_S{screen_num}"))?;
    let compositing = conn.get_selection_owner(cm)?.reply()?.owner != NONE;
    let (depth, visual, colormap, argb) = match argb_visual(&screen).filter(|_| compositing) {
        Some(v) => {
            let cmap = conn.generate_id()?;
            conn.create_colormap(ColormapAlloc::NONE, cmap, root, v)?;
            (32, v, cmap, true)
        }
        None => (screen.root_depth, screen.root_visual, screen.default_colormap, false),
    };

    let win = conn.generate_id()?;
    let aux = CreateWindowAux::new().background_pixel(0).border_pixel(0).colormap(colormap).event_mask(
        EventMask::EXPOSURE
            | EventMask::BUTTON_PRESS
            | EventMask::BUTTON_RELEASE
            | EventMask::POINTER_MOTION
            | EventMask::ENTER_WINDOW
            | EventMask::LEAVE_WINDOW,
    );
    conn.create_window(depth, win, root, 0, 0, 1, 1, 0, WindowClass::INPUT_OUTPUT, visual, &aux)?;
    let gc = conn.generate_id()?;
    conn.create_gc(gc, win, &CreateGCAux::new())?;

    conn.change_property8(PropMode::REPLACE, win, AtomEnum::WM_NAME, AtomEnum::STRING, b"Dockling")?;
    conn.change_property8(PropMode::REPLACE, win, atoms._NET_WM_NAME, atoms.UTF8_STRING, b"Dockling")?;
    conn.change_property8(PropMode::REPLACE, win, AtomEnum::WM_CLASS, AtomEnum::STRING, b"dockling\0Dockling\0")?;
    conn.change_property32(
        PropMode::REPLACE,
        win,
        atoms._NET_WM_WINDOW_TYPE,
        AtomEnum::ATOM,
        &[atoms._NET_WM_WINDOW_TYPE_DOCK],
    )?;
    conn.change_property32(
        PropMode::REPLACE,
        win,
        atoms._NET_WM_STATE,
        AtomEnum::ATOM,
        &[
            atoms._NET_WM_STATE_STICKY,
            atoms._NET_WM_STATE_ABOVE,
            atoms._NET_WM_STATE_SKIP_TASKBAR,
            atoms._NET_WM_STATE_SKIP_PAGER,
        ],
    )?;
    conn.change_property32(PropMode::REPLACE, win, atoms._NET_WM_DESKTOP, AtomEnum::CARDINAL, &[u32::MAX])?;
    conn.change_property32(PropMode::REPLACE, win, atoms._NET_WM_PID, AtomEnum::CARDINAL, &[std::process::id()])?;
    // WM_HINTS: never take the keyboard focus.
    conn.change_property32(
        PropMode::REPLACE,
        win,
        AtomEnum::WM_HINTS,
        AtomEnum::WM_HINTS,
        &[1, 0, 0, 0, 0, 0, 0, 0, 0],
    )?;

    conn.set_selection_owner(win, selection, CURRENT_TIME)?;
    if conn.get_selection_owner(selection)?.reply()?.owner != win {
        return Err("could not become the dock owner".into());
    }
    conn.change_window_attributes(
        root,
        &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE | EventMask::STRUCTURE_NOTIFY),
    )?;

    let cfg_path = config::config_path();
    let cfg = config::load(&cfg_path);
    let mut dock = Dock {
        icons: IconFinder::new(&icon_theme(&conn, screen_num, &cfg)),
        apps: desktop::load_apps(),
        conn,
        screen_num,
        root,
        atoms,
        selection,
        depth,
        visual,
        colormap,
        argb,
        win,
        gc,
        tooltip_win: None,
        menu_win: None,
        tooltip_for: None,
        menu: None,
        cfg,
        cfg_path,
        icon_cache: HashMap::new(),
        font: None,
        clients: HashMap::new(),
        order: Vec::new(),
        active: NONE,
        items: Vec::new(),
        monitor: R::default(),
        geom: R::default(),
        struts: [u32::MAX; 12],
        icon_px: 0,
        hover: None,
        pressed: None,
        drag: None,
        hidden: false,
        hide_at: None,
        children: Vec::new(),
    };
    dock.read_active();
    dock.refresh_clients()?;
    dock.conn.map_window(win)?;
    dock.start_autohide_timer();
    dock.event_loop()
}

impl Dock {
    // ---------------------------------------------------------------- X helpers

    fn prop32(&self, w: Window, prop: Atom, ty: impl Into<Atom>) -> Vec<u32> {
        self.conn
            .get_property(false, w, prop, ty, 0, u32::MAX / 4)
            .ok()
            .and_then(|c| c.reply().ok())
            .and_then(|r| r.value32().map(|v| v.collect()))
            .unwrap_or_default()
    }

    fn prop8(&self, w: Window, prop: impl Into<Atom>, ty: impl Into<Atom>) -> Vec<u8> {
        self.conn
            .get_property(false, w, prop, ty, 0, 1024)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|r| r.value)
            .unwrap_or_default()
    }

    fn send_root_message(&self, w: Window, ty: Atom, data: [u32; 5]) -> Res<()> {
        let ev = ClientMessageEvent::new(32, w, ty, data);
        self.conn.send_event(
            false,
            self.root,
            EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
            ev,
        )?;
        Ok(())
    }

    fn activate(&self, w: Window, time: Timestamp) -> Res<()> {
        self.send_root_message(w, self.atoms._NET_ACTIVE_WINDOW, [2, time, self.active, 0, 0])
    }

    fn minimize(&self, w: Window) -> Res<()> {
        self.send_root_message(w, self.atoms.WM_CHANGE_STATE, [3, 0, 0, 0, 0])
    }

    fn close(&self, w: Window, time: Timestamp) -> Res<()> {
        self.send_root_message(w, self.atoms._NET_CLOSE_WINDOW, [time, 2, 0, 0, 0])
    }

    /// Uploads a tiny-skia pixmap as the background of `win`.
    fn upload(&self, win: Window, pm: &Pixmap) -> Res<()> {
        let (w, h) = (pm.width() as u16, pm.height() as u16);
        let mut data = pm.data().to_vec();
        for px in data.as_chunks_mut::<4>().0 {
            px.swap(0, 2); // RGBA -> BGRA (little-endian ARGB32)
        }
        let image = Image::new(
            w,
            h,
            ScanlinePad::Pad32,
            self.depth,
            BitsPerPixel::B32,
            ImageOrder::LsbFirst,
            Cow::Owned(data),
        )?;
        let xpm = self.conn.generate_id()?;
        self.conn.create_pixmap(self.depth, xpm, win, w, h)?;
        image.put(&self.conn, xpm, self.gc, 0, 0)?;
        self.conn.change_window_attributes(win, &ChangeWindowAttributesAux::new().background_pixmap(xpm))?;
        self.conn.clear_area(false, win, 0, 0, 0, 0)?;
        self.conn.free_pixmap(xpm)?;
        Ok(())
    }

    fn create_popup(&self, kind: Atom) -> Res<Window> {
        let w = self.conn.generate_id()?;
        let aux = CreateWindowAux::new()
            .background_pixel(0)
            .border_pixel(0)
            .colormap(self.colormap)
            .override_redirect(1)
            .event_mask(EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION);
        self.conn.create_window(
            self.depth,
            w,
            self.root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_OUTPUT,
            self.visual,
            &aux,
        )?;
        self.conn.change_property32(PropMode::REPLACE, w, self.atoms._NET_WM_WINDOW_TYPE, AtomEnum::ATOM, &[kind])?;
        Ok(w)
    }

    fn show_popup(&self, win: Window, r: R, pm: &Pixmap) -> Res<()> {
        self.conn.configure_window(
            win,
            &ConfigureWindowAux::new().x(r.x).y(r.y).width(r.w).height(r.h).stack_mode(StackMode::ABOVE),
        )?;
        self.upload(win, pm)?;
        self.conn.map_window(win)?;
        Ok(())
    }

    fn text(&mut self) -> Option<&Text> {
        if self.font.is_none() {
            self.font = Some(Text::load(&self.cfg.font, self.cfg.font_size));
        }
        self.font.as_ref().and_then(|f| f.as_ref())
    }

    // ---------------------------------------------------------------- state

    fn read_active(&mut self) {
        self.active =
            self.prop32(self.root, self.atoms._NET_ACTIVE_WINDOW, AtomEnum::WINDOW).first().copied().unwrap_or(NONE);
    }

    fn read_client(&self, w: Window) -> Option<Client> {
        let a = &self.atoms;
        let types = self.prop32(w, a._NET_WM_WINDOW_TYPE, AtomEnum::ATOM);
        if !types.is_empty()
            && !types.iter().any(|t| *t == a._NET_WM_WINDOW_TYPE_NORMAL || *t == a._NET_WM_WINDOW_TYPE_DIALOG)
        {
            return None;
        }
        let transient = self.prop32(w, AtomEnum::WM_TRANSIENT_FOR.into(), AtomEnum::WINDOW);
        if transient.first().is_some_and(|t| *t != NONE) {
            return None;
        }
        let state = self.prop32(w, a._NET_WM_STATE, AtomEnum::ATOM);
        if state.contains(&a._NET_WM_STATE_SKIP_TASKBAR) {
            return None;
        }
        let raw = self.prop8(w, AtomEnum::WM_CLASS, AtomEnum::STRING);
        let mut parts = raw.split(|b| *b == 0).map(|s| String::from_utf8_lossy(s).into_owned());
        let instance = parts.next().unwrap_or_default();
        let class = parts.next().filter(|c| !c.is_empty()).unwrap_or_else(|| instance.clone());
        let app = desktop::match_window(&self.apps, &instance, &class);
        let (group, name, icon_hint) = match app {
            Some(i) => (self.apps[i].id.clone(), self.apps[i].name.clone(), self.apps[i].icon.clone()),
            None => {
                let lower = class.to_lowercase();
                let name = if class.is_empty() { "Unknown".into() } else { class.clone() };
                (format!("class:{lower}"), name, lower)
            }
        };
        Some(Client { group, app, name, icon_hint, attention: state.contains(&a._NET_WM_STATE_DEMANDS_ATTENTION) })
    }

    fn refresh_clients(&mut self) -> Res<()> {
        let list = self.prop32(self.root, self.atoms._NET_CLIENT_LIST, AtomEnum::WINDOW);
        self.clients.retain(|w, _| list.contains(w));
        for &w in &list {
            if w == self.win || self.clients.contains_key(&w) {
                continue;
            }
            // Follow state changes (attention, skip-taskbar, class) of every client.
            self.conn.change_window_attributes(
                w,
                &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
            )?;
            let client = self.read_client(w);
            self.clients.insert(w, client);
        }
        self.order = list;
        self.rebuild_items()
    }

    fn rebuild_items(&mut self) -> Res<()> {
        if self.dragging() {
            // Keep the order being dragged; finish_drag() rebuilds afterwards.
            return Ok(());
        }
        let mut items = Vec::new();
        let apps_button = Item {
            kind: Kind::AppsButton,
            key: "apps-button".into(),
            app: None,
            name: "Show Applications".into(),
            icon_hint: String::new(),
            pinned: true,
            windows: Vec::new(),
            rect: R::default(),
        };
        let pinned = self.cfg.pinned.clone();
        for entry in &pinned {
            let Some(i) = desktop::resolve(&mut self.apps, entry) else {
                continue;
            };
            let app = &self.apps[i];
            if items.iter().any(|it: &Item| it.key == app.id) {
                continue;
            }
            items.push(Item {
                kind: Kind::App,
                key: app.id.clone(),
                app: Some(i),
                name: app.name.clone(),
                icon_hint: app.icon.clone(),
                pinned: true,
                windows: Vec::new(),
                rect: R::default(),
            });
        }
        for &w in &self.order {
            let Some(Some(c)) = self.clients.get(&w) else {
                continue;
            };
            if let Some(item) = items.iter_mut().find(|it| it.key == c.group) {
                item.windows.push(w);
            } else if self.cfg.show_running {
                items.push(Item {
                    kind: Kind::App,
                    key: c.group.clone(),
                    app: c.app,
                    name: c.name.clone(),
                    icon_hint: c.icon_hint.clone(),
                    pinned: false,
                    windows: vec![w],
                    rect: R::default(),
                });
            }
        }
        if self.cfg.show_apps_button {
            if self.cfg.apps_button_at_start {
                items.insert(0, apps_button);
            } else {
                items.push(apps_button);
            }
        }
        self.icon_cache.retain(|k, _| items.iter().any(|it| it.key == *k));
        self.items = items;
        if self.hover.as_ref().is_some_and(|h| !self.items.iter().any(|it| it.key == *h)) {
            self.hover = None;
            self.hide_tooltip()?;
        }
        self.layout()?;
        self.render()
    }

    fn item_index(&self, key: &str) -> Option<usize> {
        self.items.iter().position(|it| it.key == key)
    }

    fn item_at(&self, x: i32, y: i32) -> Option<usize> {
        let horizontal = self.cfg.position.horizontal();
        self.items
            .iter()
            .position(|it| {
                let r = it.rect;
                // Anything across the dock's thickness counts, so edges are easy to hit.
                if horizontal { x >= r.x && x < r.x + r.w as i32 } else { y >= r.y && y < r.y + r.h as i32 }
            })
            .filter(|_| !self.hidden && R { x: 0, y: 0, w: self.geom.w, h: self.geom.h }.contains(x, y))
    }

    // ---------------------------------------------------------------- geometry

    fn read_monitor(&self) -> R {
        let root_geom = self
            .conn
            .get_geometry(self.root)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|g| R { x: 0, y: 0, w: g.width as u32, h: g.height as u32 })
            .unwrap_or(R { x: 0, y: 0, w: 1024, h: 768 });
        let has_randr = self.conn.extension_information(randr::X11_EXTENSION_NAME).ok().flatten().is_some();
        if !has_randr {
            return root_geom;
        }
        let monitors = self
            .conn
            .randr_get_monitors(self.root, true)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|r| r.monitors)
            .unwrap_or_default();
        monitors
            .iter()
            .find(|m| m.primary)
            .or_else(|| monitors.first())
            .map(|m| R { x: m.x as i32, y: m.y as i32, w: m.width as u32, h: m.height as u32 })
            .filter(|m| m.w > 0 && m.h > 0)
            .unwrap_or(root_geom)
    }

    /// Space taken by other panels at the start/end of the dock's edge, read from
    /// _NET_WORKAREA (our own strut only affects the other axis).
    fn panel_offsets(&self, mon: R) -> (u32, u32) {
        let desktop =
            self.prop32(self.root, self.atoms._NET_CURRENT_DESKTOP, AtomEnum::CARDINAL).first().copied().unwrap_or(0)
                as usize;
        let wa = self.prop32(self.root, self.atoms._NET_WORKAREA, AtomEnum::CARDINAL);
        let Some(wa) = wa.get(desktop * 4..desktop * 4 + 4).or_else(|| wa.get(0..4)) else {
            return (0, 0);
        };
        let (wx, wy, ww, wh) = (wa[0] as i64, wa[1] as i64, wa[2] as i64, wa[3] as i64);
        let (start, end, len) = if self.cfg.position.horizontal() {
            (wx - mon.x as i64, (mon.x as i64 + mon.w as i64) - (wx + ww), mon.w as i64)
        } else {
            (wy - mon.y as i64, (mon.y as i64 + mon.h as i64) - (wy + wh), mon.h as i64)
        };
        let clamp = |v: i64| v.clamp(0, len / 3) as u32;
        (clamp(start), clamp(end))
    }

    fn layout(&mut self) -> Res<()> {
        let mon = self.read_monitor();
        self.monitor = mon;
        let (s0, s1) = self.panel_offsets(mon);
        let cfg = &self.cfg;
        let horizontal = cfg.position.horizontal();
        let avail = (if horizontal { mon.w } else { mon.h }).saturating_sub(s0 + s1) as i64;
        let n = self.items.len().max(1) as i64;
        let full_cell = (cfg.icon_size + 2 * cfg.icon_padding) as i64;
        let (sp, m) = (cfg.spacing as i64, cfg.margin as i64);
        let need = |cell: i64| n * cell + (n - 1) * sp + 2 * m;
        let mut cell = full_cell;
        if need(cell) > avail {
            cell = ((avail - 2 * m - (n - 1) * sp) / n).max(16);
        }
        let thickness = full_cell + 2 * m;
        let length = if cfg.extend { avail } else { need(cell).min(avail) }.max(1);
        let start = s0 as i64 + if cfg.extend { 0 } else { (avail - length) / 2 };

        let icon_px = ((cell * cfg.icon_size as i64) / full_cell).max(8) as u32;
        if icon_px != self.icon_px {
            self.icon_px = icon_px;
            self.icon_cache.clear();
        }

        let cross = (thickness - cell) / 2;
        let apps_at_end = cfg.extend && !cfg.apps_button_at_start;
        for (i, item) in self.items.iter_mut().enumerate() {
            let along = if item.kind == Kind::AppsButton && apps_at_end {
                length - m - cell
            } else {
                m + i as i64 * (cell + sp)
            };
            let (c, a) = (cross as i32, along as i32);
            item.rect = if horizontal {
                R { x: a, y: c, w: cell as u32, h: cell as u32 }
            } else {
                R { x: c, y: a, w: cell as u32, h: cell as u32 }
            };
        }

        let thick = if self.hidden { HIDDEN_THICKNESS } else { thickness as u32 };
        let (len, start) = (length as u32, start as i32);
        let g = match cfg.position {
            Position::Left => R { x: mon.x, y: mon.y + start, w: thick, h: len },
            Position::Right => R { x: mon.x + mon.w as i32 - thick as i32, y: mon.y + start, w: thick, h: len },
            Position::Top => R { x: mon.x + start, y: mon.y, w: len, h: thick },
            Position::Bottom => R { x: mon.x + start, y: mon.y + mon.h as i32 - thick as i32, w: len, h: thick },
        };
        if g != self.geom {
            self.geom = g;
            self.conn.configure_window(self.win, &ConfigureWindowAux::new().x(g.x).y(g.y).width(g.w).height(g.h))?;
            // WM_NORMAL_HINTS: user position/size, fixed size.
            let mut hints = [0u32; 18];
            hints[0] = 1 | 2 | 4 | 8 | 16 | 32;
            hints[1..5].copy_from_slice(&[g.x as u32, g.y as u32, g.w, g.h]);
            hints[5..9].copy_from_slice(&[g.w, g.h, g.w, g.h]);
            self.conn.change_property32(
                PropMode::REPLACE,
                self.win,
                AtomEnum::WM_NORMAL_HINTS,
                AtomEnum::WM_SIZE_HINTS,
                &hints,
            )?;
        }
        self.set_struts(g)
    }

    fn set_struts(&mut self, g: R) -> Res<()> {
        let mut s = [0u32; 12];
        if self.cfg.reserve_space && !self.cfg.autohide {
            let root = self.conn.get_geometry(self.root)?.reply().map(|r| (r.width as i32, r.height as i32))?;
            let (x0, x1) = (g.x.max(0) as u32, (g.x + g.w as i32 - 1).max(0) as u32);
            let (y0, y1) = (g.y.max(0) as u32, (g.y + g.h as i32 - 1).max(0) as u32);
            match self.cfg.position {
                Position::Left => (s[0], s[4], s[5]) = ((g.x + g.w as i32) as u32, y0, y1),
                Position::Right => (s[1], s[6], s[7]) = ((root.0 - g.x) as u32, y0, y1),
                Position::Top => (s[2], s[8], s[9]) = ((g.y + g.h as i32) as u32, x0, x1),
                Position::Bottom => (s[3], s[10], s[11]) = ((root.1 - g.y) as u32, x0, x1),
            }
        }
        // Only touch the properties when they change: the WM answers with a
        // _NET_WORKAREA update, which would otherwise loop back here.
        if s != self.struts {
            self.struts = s;
            let a = &self.atoms;
            self.conn.change_property32(
                PropMode::REPLACE,
                self.win,
                a._NET_WM_STRUT_PARTIAL,
                AtomEnum::CARDINAL,
                &s,
            )?;
            self.conn.change_property32(PropMode::REPLACE, self.win, a._NET_WM_STRUT, AtomEnum::CARDINAL, &s[..4])?;
        }
        Ok(())
    }

    // ---------------------------------------------------------------- drawing

    fn ensure_icons(&mut self) {
        let px = self.icon_px;
        for i in 0..self.items.len() {
            let item = &self.items[i];
            if item.kind != Kind::App || self.icon_cache.contains_key(&item.key) {
                continue;
            }
            let path = self.icons.find(&item.icon_hint, px);
            if debug() {
                eprintln!("dockling: icon for {:?} ({:?}): {:?}", item.key, item.icon_hint, path);
            }
            let mut icon = path.and_then(|p| icons::load_icon(&p, px));
            if icon.is_none() {
                icon = item.windows.iter().find_map(|&w| {
                    let data = self.prop32(w, self.atoms._NET_WM_ICON, AtomEnum::CARDINAL);
                    icons::from_net_wm_icon(&data, px)
                });
            }
            if icon.is_none() && !item.windows.is_empty() {
                icon = self.icons.find("application-x-executable", px).and_then(|p| icons::load_icon(&p, px));
            }
            // Retry later for launchers whose window might bring an icon.
            if icon.is_some() || !item.windows.is_empty() {
                let key = item.key.clone();
                self.icon_cache.insert(key, icon);
            }
        }
    }

    fn render(&mut self) -> Res<()> {
        self.ensure_icons();
        let g = self.geom;
        let Some(mut pm) = Pixmap::new(g.w.max(1), g.h.max(1)) else {
            return Ok(());
        };
        let c = self.cfg.colors.clone();
        let bg = if self.argb { c.background } else { c.background.opaque() };
        let dock_radius = if self.argb && !self.cfg.extend { self.cfg.corner_radius } else { 0.0 };
        fill_rrect(&mut pm, R { x: 0, y: 0, w: g.w, h: g.h }, dock_radius, bg);
        if self.hidden {
            return self.upload(self.win, &pm);
        }

        let px = self.icon_px;
        let mut placeholders = Vec::new();
        let dragged = self.drag.as_ref().filter(|d| d.active).map(|d| d.key.clone());
        for item in &self.items {
            let r = item.rect;
            if dragged.as_deref() == Some(item.key.as_str()) {
                // The empty slot where the dragged icon will land.
                fill_rrect(&mut pm, r, self.cfg.corner_radius * r.w as f32 / 60.0, c.hover);
                continue;
            }
            let is_active = item.windows.contains(&self.active);
            let hovered = self.hover.as_deref() == Some(item.key.as_str());
            let highlight = match (is_active, hovered) {
                (true, _) => Some(c.active),
                (false, true) => Some(c.hover),
                _ => None,
            };
            if let Some(color) = highlight {
                fill_rrect(&mut pm, r, self.cfg.corner_radius * r.w as f32 / 60.0, color);
            }
            let ix = r.x + (r.w as i32 - px as i32) / 2;
            let iy = r.y + (r.h as i32 - px as i32) / 2;
            match item.kind {
                Kind::AppsButton => draw_grid(&mut pm, R { x: ix, y: iy, w: px, h: px }, c.apps_button),
                Kind::App => match self.icon_cache.get(&item.key).and_then(|i| i.as_ref()) {
                    Some(icon) => {
                        pm.draw_pixmap(ix, iy, icon.as_ref(), &PixmapPaint::default(), Transform::identity(), None)
                    }
                    None => placeholders.push((R { x: ix + 2, y: iy + 2, w: px - 4, h: px - 4 }, item.name.clone())),
                },
            }
            self.draw_dots(&mut pm, item, &c);
        }
        if let Some(i) = dragged.as_deref().and_then(|k| self.item_index(k)) {
            let item = &self.items[i];
            let (x, y) = self.drag_icon_origin(item.rect);
            match self.icon_cache.get(&item.key).and_then(|i| i.as_ref()) {
                Some(icon) => pm.draw_pixmap(x, y, icon.as_ref(), &PixmapPaint::default(), Transform::identity(), None),
                None => placeholders.push((R { x: x + 2, y: y + 2, w: px - 4, h: px - 4 }, item.name.clone())),
            }
        }
        // Applications without any icon: a tile with their initial.
        for (r, name) in placeholders {
            fill_rrect(&mut pm, r, r.w as f32 / 5.0, Color { r: 0x5e, g: 0x5c, b: 0x64, a: 0xff });
            let initial: String = name.chars().next().unwrap_or('?').to_uppercase().collect();
            if let Some(text) = self.text() {
                let size = r.h as f32 * 0.55;
                let tw = text.width_at(&initial, size) as i32;
                let (tx, ty) = (r.x + (r.w as i32 - tw) / 2, r.y + (r.h as f32 - size * 1.15) as i32 / 2);
                text.draw_at(&mut pm, tx, ty, &initial, c.text, r.w, size);
            }
        }
        self.upload(self.win, &pm)
    }

    /// Running indicators on the screen-edge side of the icon (one dot per window).
    fn draw_dots(&self, pm: &mut Pixmap, item: &Item, c: &config::Colors) {
        let n = item.windows.len().min(MAX_DOTS);
        if n == 0 {
            return;
        }
        let attention = item.windows.iter().any(|w| matches!(self.clients.get(w), Some(Some(cl)) if cl.attention));
        let color = if attention { c.attention } else { c.indicator };
        let r = item.rect;
        let pad = (r.w.saturating_sub(self.icon_px)) as f32 / 2.0;
        let d = (pad * 0.85).clamp(4.0, 7.0);
        let gap = d * 0.8;
        let span = n as f32 * d + (n - 1) as f32 * gap;
        let edge = (pad / 2.0).max(d / 2.0 + 1.0); // distance of the dot centers from the cell edge
        for i in 0..n {
            let along = -span / 2.0 + d / 2.0 + i as f32 * (d + gap);
            let (cx, cy) = match self.cfg.position {
                Position::Left => (r.x as f32 + edge, r.y as f32 + r.h as f32 / 2.0 + along),
                Position::Right => ((r.x + r.w as i32) as f32 - edge, r.y as f32 + r.h as f32 / 2.0 + along),
                Position::Bottom => (r.x as f32 + r.w as f32 / 2.0 + along, (r.y + r.h as i32) as f32 - edge),
                Position::Top => (r.x as f32 + r.w as f32 / 2.0 + along, r.y as f32 + edge),
            };
            if let Some(path) = PathBuilder::from_circle(cx, cy, d / 2.0) {
                pm.fill_path(&path, &paint(color), FillRule::Winding, Transform::identity(), None);
            }
        }
    }

    // ---------------------------------------------------------------- popups

    /// Places a popup next to an item, on the inner side of the dock.
    fn popup_rect(&self, item: R, w: u32, h: u32) -> R {
        let g = self.geom;
        let gap = 8;
        let cx = g.x + item.x + item.w as i32 / 2;
        let cy = g.y + item.y + item.h as i32 / 2;
        let (w_, h_) = (w as i32, h as i32);
        let (x, y) = match self.cfg.position {
            Position::Left => (g.x + g.w as i32 + gap, cy - h_ / 2),
            Position::Right => (g.x - gap - w_, cy - h_ / 2),
            Position::Bottom => (cx - w_ / 2, g.y - gap - h_),
            Position::Top => (cx - w_ / 2, g.y + g.h as i32 + gap),
        };
        let mon = self.monitor;
        let x = x.clamp(mon.x, (mon.x + mon.w as i32 - w_).max(mon.x));
        let y = y.clamp(mon.y, (mon.y + mon.h as i32 - h_).max(mon.y));
        R { x, y, w, h }
    }

    fn show_tooltip(&mut self, idx: usize) -> Res<()> {
        if !self.cfg.tooltips || self.menu.is_some() {
            return Ok(());
        }
        let key = self.items[idx].key.clone();
        if self.tooltip_for.as_deref() == Some(key.as_str()) {
            return Ok(());
        }
        let label = self.items[idx].name.clone();
        let rect = self.items[idx].rect;
        let colors = self.cfg.colors.clone();
        let argb = self.argb;
        let Some(text) = self.text() else {
            return Ok(());
        };
        let (pad_x, pad_y) = (10, 6);
        let tw = text.width(&label).min(480);
        let (w, h) = (tw + 2 * pad_x, text.line_height() + 2 * pad_y);
        let Some(mut pm) = Pixmap::new(w, h) else {
            return Ok(());
        };
        let bg = if argb { colors.tooltip_background } else { colors.tooltip_background.opaque() };
        fill_rrect(&mut pm, R { x: 0, y: 0, w, h }, if argb { 6.0 } else { 0.0 }, bg);
        text.draw(&mut pm, pad_x as i32, pad_y as i32, &label, colors.text, tw);
        let r = self.popup_rect(rect, w, h);
        let win = match self.tooltip_win {
            Some(win) => win,
            None => {
                let win = self.create_popup(self.atoms._NET_WM_WINDOW_TYPE_TOOLTIP)?;
                self.tooltip_win = Some(win);
                win
            }
        };
        self.show_popup(win, r, &pm)?;
        self.tooltip_for = Some(key);
        Ok(())
    }

    fn hide_tooltip(&mut self) -> Res<()> {
        if let (Some(win), Some(_)) = (self.tooltip_win, self.tooltip_for.take()) {
            self.conn.unmap_window(win)?;
        }
        Ok(())
    }

    /// Opens the menu of an item, or the dock's own menu (`idx` = None) next to `at`.
    fn open_menu(&mut self, idx: Option<usize>, at: (i32, i32)) -> Res<()> {
        self.hide_tooltip()?;
        let dock_entries =
            || vec![("Dock Settings".into(), Action::OpenSettings), ("Reload Dock".into(), Action::Reload)];
        let mut entries: Vec<(String, Action)> = Vec::new();
        let rect = match idx {
            Some(i) => self.items[i].rect,
            None => R { x: at.0, y: at.1, w: 1, h: 1 },
        };
        match idx.map(|i| &self.items[i]) {
            None => entries = dock_entries(),
            Some(item) if item.kind == Kind::AppsButton => entries = dock_entries(),
            Some(item) => {
                if item.app.is_some() {
                    let label = if item.windows.is_empty() { "Open" } else { "New Window" };
                    entries.push((label.into(), Action::Launch(item.key.clone())));
                    let label = if item.pinned { "Unpin from Dock" } else { "Pin to Dock" };
                    entries.push((label.into(), Action::TogglePin(item.key.clone())));
                }
                match item.windows.len() {
                    0 => {}
                    1 => entries.push(("Quit".into(), Action::CloseAll(item.key.clone()))),
                    n => entries.push((format!("Quit {n} Windows"), Action::CloseAll(item.key.clone()))),
                }
                entries.push(("Dock Settings".into(), Action::OpenSettings));
            }
        }
        if entries.is_empty() {
            return Ok(());
        }
        let Some(text) = self.text() else {
            return Ok(());
        };
        let row_h = text.line_height() + 12;
        let w = entries.iter().map(|(l, _)| text.width(l)).max().unwrap_or(0).max(120) + 32;
        let h = entries.len() as u32 * row_h + 2 * MENU_PAD;
        let r = self.popup_rect(rect, w, h);
        let win = match self.menu_win {
            Some(win) => win,
            None => {
                let win = self.create_popup(self.atoms._NET_WM_WINDOW_TYPE_POPUP_MENU)?;
                self.menu_win = Some(win);
                win
            }
        };
        self.menu = Some(Menu { rect: r, entries, hover: None, row_h });
        self.draw_menu()?;
        // Grab the pointer so a click anywhere else closes the menu.
        self.conn.grab_pointer(
            false,
            win,
            EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION,
            GrabMode::ASYNC,
            GrabMode::ASYNC,
            NONE,
            NONE,
            CURRENT_TIME,
        )?;
        Ok(())
    }

    fn draw_menu(&mut self) -> Res<()> {
        let (Some(win), Some(menu)) = (self.menu_win, self.menu.as_ref()) else {
            return Ok(());
        };
        let r = menu.rect;
        let rows: Vec<(String, bool)> =
            menu.entries.iter().enumerate().map(|(i, (l, _))| (l.clone(), menu.hover == Some(i))).collect();
        let row_h = menu.row_h;
        let colors = self.cfg.colors.clone();
        let argb = self.argb;
        let Some(text) = self.text() else {
            return Ok(());
        };
        let Some(mut pm) = Pixmap::new(r.w, r.h) else {
            return Ok(());
        };
        let bg = if argb { colors.menu_background } else { colors.menu_background.opaque() };
        fill_rrect(&mut pm, R { x: 0, y: 0, w: r.w, h: r.h }, if argb { 8.0 } else { 0.0 }, bg);
        for (i, (label, hovered)) in rows.iter().enumerate() {
            let y = (MENU_PAD + i as u32 * row_h) as i32;
            if *hovered {
                fill_rrect(&mut pm, R { x: 4, y, w: r.w - 8, h: row_h }, 5.0, colors.menu_hover);
            }
            let ty = y + (row_h as i32 - text.line_height() as i32) / 2;
            text.draw(&mut pm, 16, ty, label, colors.text, r.w - 32);
        }
        self.show_popup(win, r, &pm)
    }

    fn close_menu(&mut self) -> Res<()> {
        if self.menu.take().is_some() {
            self.conn.ungrab_pointer(CURRENT_TIME)?;
            if let Some(win) = self.menu_win {
                self.conn.unmap_window(win)?;
            }
            if !self.pointer_inside()? {
                self.hover = None;
                self.start_autohide_timer();
                self.render()?;
            }
        }
        Ok(())
    }

    fn menu_row(&self, x: i32, y: i32) -> Option<usize> {
        let menu = self.menu.as_ref()?;
        if x < 0 || x >= menu.rect.w as i32 || y < MENU_PAD as i32 {
            return None;
        }
        let row = ((y - MENU_PAD as i32) / menu.row_h as i32) as usize;
        (row < menu.entries.len()).then_some(row)
    }

    fn pointer_inside(&self) -> Res<bool> {
        let p = self.conn.query_pointer(self.root)?.reply()?;
        let g = self.geom;
        Ok(R { x: g.x, y: g.y, w: g.w, h: g.h }.contains(p.root_x as i32, p.root_y as i32))
    }

    // ---------------------------------------------------------------- actions

    fn spawn(&mut self, argv: &[String]) {
        let Some((prog, args)) = argv.split_first() else {
            return;
        };
        let mut cmd = Command::new(prog);
        cmd.args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .current_dir(config::home_dir())
            .process_group(0);
        match cmd.spawn() {
            Ok(child) => self.children.push(child),
            Err(e) => eprintln!("dockling: cannot start {prog}: {e}"),
        }
    }

    fn launch_app(&mut self, app: usize) {
        let app = &self.apps[app];
        let argv = if app.terminal {
            let mut v = vec![self.cfg.terminal.clone(), "-e".into()];
            v.extend(app.exec.iter().cloned());
            v
        } else {
            app.exec.clone()
        };
        self.spawn(&argv);
    }

    fn run_shell(&mut self, command: &str) {
        self.spawn(&["sh".into(), "-c".into(), command.into()]);
    }

    /// Windows of an item ordered from the topmost down.
    fn stacked_windows(&self, item: &Item) -> Vec<Window> {
        let stack = self.prop32(self.root, self.atoms._NET_CLIENT_LIST_STACKING, AtomEnum::WINDOW);
        let mut wins = item.windows.clone();
        wins.sort_by_key(|w| std::cmp::Reverse(stack.iter().position(|s| s == w)));
        wins
    }

    fn primary_click(&mut self, idx: usize, time: Timestamp) -> Res<()> {
        let item = &self.items[idx];
        if item.kind == Kind::AppsButton {
            let cmd = self.cfg.apps_command.clone();
            self.run_shell(&cmd);
            return Ok(());
        }
        if item.windows.is_empty() {
            if let Some(app) = item.app {
                self.launch_app(app);
            }
            return Ok(());
        }
        if let Some(pos) = item.windows.iter().position(|w| *w == self.active) {
            if item.windows.len() == 1 {
                if self.cfg.click_action == ClickAction::MinimizeOrCycle {
                    self.minimize(self.active)?;
                }
            } else {
                let next = item.windows[(pos + 1) % item.windows.len()];
                self.activate(next, time)?;
            }
        } else {
            let top = self.stacked_windows(item)[0];
            self.activate(top, time)?;
        }
        Ok(())
    }

    fn scroll(&mut self, idx: usize, forward: bool, time: Timestamp) -> Res<()> {
        let wins = &self.items[idx].windows;
        if wins.is_empty() {
            return Ok(());
        }
        let n = wins.len();
        let target = match wins.iter().position(|w| *w == self.active) {
            Some(pos) if forward => wins[(pos + 1) % n],
            Some(pos) => wins[(pos + n - 1) % n],
            None => wins[0],
        };
        self.activate(target, time)
    }

    fn run_action(&mut self, action: Action, time: Timestamp) -> Res<()> {
        match action {
            Action::Launch(key) => {
                if let Some(app) = self.item_index(&key).and_then(|i| self.items[i].app) {
                    self.launch_app(app);
                }
            }
            Action::CloseAll(key) => {
                if let Some(i) = self.item_index(&key) {
                    for &w in &self.items[i].windows {
                        self.close(w, time)?;
                    }
                }
            }
            Action::TogglePin(key) => self.toggle_pin(&key)?,
            Action::OpenSettings => {
                // The settings window next to this binary, else in PATH, else the file itself.
                let beside = std::env::current_exe()
                    .ok()
                    .and_then(|p| Some(p.parent()?.join("dockling-settings")))
                    .filter(|p| p.is_file());
                match beside {
                    Some(settings) => self.spawn(&[settings.to_string_lossy().into_owned()]),
                    None => {
                        let path = format!("'{}'", self.cfg_path.to_string_lossy().replace('\'', "'\\''"));
                        self.run_shell(&format!(
                            "if command -v dockling-settings >/dev/null; then exec dockling-settings; \
                             else exo-open {path} 2>/dev/null || xdg-open {path}; fi"
                        ));
                    }
                }
            }
            Action::Reload => self.reload()?,
        }
        Ok(())
    }

    fn toggle_pin(&mut self, key: &str) -> Res<()> {
        let Some(app) = self.item_index(key).and_then(|i| self.items[i].app) else {
            return Ok(());
        };
        let mut pinned = self.cfg.pinned.clone();
        let before = pinned.len();
        let mut apps = std::mem::take(&mut self.apps);
        pinned.retain(|entry| desktop::resolve(&mut apps, entry) != Some(app));
        if pinned.len() == before {
            pinned.push(apps[app].id.clone());
        }
        self.apps = apps;
        if let Err(e) = config::save_pinned(&self.cfg_path, &pinned) {
            eprintln!("dockling: cannot save {}: {e}", self.cfg_path.display());
        }
        self.cfg.pinned = pinned;
        self.rebuild_items()
    }

    fn reload(&mut self) -> Res<()> {
        self.close_menu()?;
        self.hide_tooltip()?;
        self.cfg = config::load(&self.cfg_path);
        self.apps = desktop::load_apps();
        self.icons = IconFinder::new(&icon_theme(&self.conn, self.screen_num, &self.cfg));
        self.icon_cache.clear();
        self.font = None;
        self.clients.clear();
        self.hidden = false;
        self.hide_at = None;
        self.geom = R::default();
        self.refresh_clients()?;
        self.start_autohide_timer();
        Ok(())
    }

    fn start_autohide_timer(&mut self) {
        if self.cfg.autohide && !self.hidden {
            self.hide_at = Some(Instant::now() + Duration::from_millis(self.cfg.autohide_delay_ms));
        }
    }

    fn set_hidden(&mut self, hidden: bool) -> Res<()> {
        if self.hidden != hidden {
            self.hidden = hidden;
            self.layout()?;
            self.render()?;
        }
        Ok(())
    }

    // ---------------------------------------------------------------- drag and drop

    fn dragging(&self) -> bool {
        self.drag.as_ref().is_some_and(|d| d.active)
    }

    /// Top-left corner of the dragged icon: it follows the pointer along the dock.
    fn drag_icon_origin(&self, slot: R) -> (i32, i32) {
        let px = self.icon_px as i32;
        let (x, y) = self.drag.as_ref().map(|d| d.pos).unwrap_or_default();
        let (ix, iy) = (slot.x + (slot.w as i32 - px) / 2, slot.y + (slot.h as i32 - px) / 2);
        if self.cfg.position.horizontal() {
            ((x - px / 2).clamp(0, (self.geom.w as i32 - px).max(0)), iy)
        } else {
            (ix, (y - px / 2).clamp(0, (self.geom.h as i32 - px).max(0)))
        }
    }

    fn on_drag_motion(&mut self, x: i32, y: i32) -> Res<()> {
        let Some(drag) = self.drag.as_mut() else { return Ok(()) };
        drag.pos = (x, y);
        let key = drag.key.clone();
        if !drag.active {
            let (dx, dy) = (x - drag.origin.0, y - drag.origin.1);
            if dx.abs().max(dy.abs()) < DRAG_THRESHOLD {
                return Ok(());
            }
            drag.active = true;
            self.pressed = None;
            self.hover = None;
            self.hide_tooltip()?;
        }
        let pinned = |it: &Item| it.kind == Kind::App && it.pinned;
        let (Some(from), Some(first), Some(last)) =
            (self.item_index(&key), self.items.iter().position(pinned), self.items.iter().rposition(pinned))
        else {
            return Ok(());
        };
        // Pinned icons sit at regular steps from the start of the dock.
        let step = (self.items[from].rect.w + self.cfg.spacing) as i32;
        let along = if self.cfg.position.horizontal() { x } else { y } - self.cfg.margin as i32;
        let slot = (along.max(0) / step.max(1)) as usize;
        let slot = slot.clamp(first, last);
        if slot != from {
            let item = self.items.remove(from);
            self.items.insert(slot, item);
            self.layout()?;
        }
        self.render()
    }

    /// Saves the new order of the pinned apps, keeping the user's own spelling of each
    /// entry and the position of entries that are not shown (apps not installed).
    fn finish_drag(&mut self) -> Res<()> {
        self.drag = None;
        let visual: Vec<usize> =
            self.items.iter().filter(|it| it.kind == Kind::App && it.pinned).filter_map(|it| it.app).collect();
        let mut apps = std::mem::take(&mut self.apps);
        let resolved: Vec<Option<usize>> =
            self.cfg.pinned.iter().map(|entry| desktop::resolve(&mut apps, entry)).collect();
        self.apps = apps;
        let mut ordered = visual.iter().filter_map(|app| {
            let i = resolved.iter().position(|r| *r == Some(*app))?;
            Some(self.cfg.pinned[i].clone())
        });
        let mut pinned = Vec::new();
        for (entry, app) in self.cfg.pinned.iter().zip(&resolved) {
            match app {
                Some(app) if visual.contains(app) => pinned.extend(ordered.next()),
                _ => pinned.push(entry.clone()),
            }
        }
        if pinned != self.cfg.pinned {
            if let Err(e) = config::save_pinned(&self.cfg_path, &pinned) {
                eprintln!("dockling: cannot save {}: {e}", self.cfg_path.display());
            }
            self.cfg.pinned = pinned;
        }
        if !self.pointer_inside()? {
            self.hover = None;
            self.start_autohide_timer();
        }
        self.rebuild_items()
    }

    // ---------------------------------------------------------------- events

    fn on_motion(&mut self, x: i32, y: i32) -> Res<()> {
        let idx = self.item_at(x, y);
        let key = idx.map(|i| self.items[i].key.clone());
        if key != self.hover {
            self.hover = key;
            self.render()?;
            match idx {
                Some(i) => {
                    self.tooltip_for = None;
                    self.show_tooltip(i)?;
                }
                None => self.hide_tooltip()?,
            }
        }
        Ok(())
    }

    /// Returns false when the dock should exit.
    fn handle(&mut self, event: Event) -> Res<bool> {
        let a = self.atoms;
        match event {
            Event::MotionNotify(e) if self.menu.is_some() => {
                let row = self.menu_row(e.event_x as i32, e.event_y as i32).filter(|_| self.menu_win == Some(e.event));
                if self.menu.as_ref().is_some_and(|m| m.hover != row) {
                    if let Some(m) = self.menu.as_mut() {
                        m.hover = row;
                    }
                    self.draw_menu()?;
                }
            }
            Event::MotionNotify(e) if e.event == self.win && self.drag.is_some() => {
                self.on_drag_motion(e.event_x as i32, e.event_y as i32)?
            }
            Event::MotionNotify(e) if e.event == self.win => self.on_motion(e.event_x as i32, e.event_y as i32)?,
            Event::EnterNotify(e) if e.event == self.win => {
                self.hide_at = None;
                self.set_hidden(false)?;
                self.on_motion(e.event_x as i32, e.event_y as i32)?;
            }
            Event::LeaveNotify(e) if e.event == self.win && self.menu.is_none() && !self.dragging() => {
                if e.mode == NotifyMode::NORMAL && e.detail != NotifyDetail::INFERIOR {
                    self.hover = None;
                    self.hide_tooltip()?;
                    self.render()?;
                    self.start_autohide_timer();
                }
            }
            Event::ButtonPress(e) if self.menu.is_some() => {
                let inside = self.menu_win == Some(e.event)
                    && self.menu.as_ref().is_some_and(|m| {
                        R { x: 0, y: 0, w: m.rect.w, h: m.rect.h }.contains(e.event_x as i32, e.event_y as i32)
                    });
                if !inside {
                    self.close_menu()?;
                }
            }
            Event::ButtonRelease(e) if self.menu.is_some() => {
                // Releases still addressed to the dock (the click that opened the menu) are ignored.
                let row = self.menu_row(e.event_x as i32, e.event_y as i32).filter(|_| self.menu_win == Some(e.event));
                if let Some(row) = row {
                    let action = self.menu.as_ref().map(|m| m.entries[row].1.clone());
                    self.close_menu()?;
                    if let Some(action) = action {
                        self.run_action(action, e.time)?;
                    }
                }
            }
            Event::ButtonPress(e) if e.event == self.win => {
                let at = (e.event_x as i32, e.event_y as i32);
                let Some(idx) = self.item_at(at.0, at.1) else {
                    // Empty part of the dock: the dock's own menu.
                    if e.detail == 3 {
                        self.open_menu(None, at)?;
                    }
                    return Ok(true);
                };
                self.hide_tooltip()?;
                match e.detail {
                    3 => self.open_menu(Some(idx), at)?,
                    4 | 6 => self.scroll(idx, false, e.time)?,
                    5 | 7 => self.scroll(idx, true, e.time)?,
                    b => {
                        let item = &self.items[idx];
                        if b == 1 && item.kind == Kind::App && item.pinned {
                            let at = (e.event_x as i32, e.event_y as i32);
                            self.drag = Some(Drag { key: item.key.clone(), origin: at, pos: at, active: false });
                        }
                        self.pressed = Some((self.items[idx].key.clone(), b));
                    }
                }
            }
            Event::ButtonRelease(e) if e.event == self.win => {
                if e.detail == 1 && self.dragging() {
                    self.finish_drag()?;
                    return Ok(true);
                }
                self.drag = None;
                let Some((key, button)) = self.pressed.take() else {
                    return Ok(true);
                };
                let Some(idx) = self.item_at(e.event_x as i32, e.event_y as i32) else {
                    return Ok(true);
                };
                if self.items[idx].key != key || button != e.detail {
                    return Ok(true);
                }
                match button {
                    1 => self.primary_click(idx, e.time)?,
                    2 => match self.items[idx].app {
                        Some(app) => self.launch_app(app),
                        None => self.primary_click(idx, e.time)?,
                    },
                    _ => {}
                }
            }
            Event::PropertyNotify(e) if e.window == self.root => {
                if e.atom == a._NET_CLIENT_LIST {
                    self.refresh_clients()?;
                } else if e.atom == a._NET_ACTIVE_WINDOW {
                    self.read_active();
                    self.render()?;
                } else if e.atom == a._NET_WORKAREA || e.atom == a._NET_CURRENT_DESKTOP {
                    self.layout()?;
                    self.render()?;
                }
            }
            Event::PropertyNotify(e) if self.clients.contains_key(&e.window) => {
                if e.atom == a._NET_WM_STATE
                    || e.atom == a._NET_WM_WINDOW_TYPE
                    || e.atom == u32::from(AtomEnum::WM_CLASS)
                {
                    let client = self.read_client(e.window);
                    self.clients.insert(e.window, client);
                    self.rebuild_items()?;
                } else if e.atom == a._NET_WM_ICON
                    && let Some(Some(c)) = self.clients.get(&e.window)
                    && c.app.is_none()
                {
                    let group = c.group.clone();
                    self.icon_cache.remove(&group);
                    self.render()?;
                }
            }
            Event::ConfigureNotify(e) if e.window == self.root => {
                self.layout()?;
                self.render()?;
            }
            Event::ClientMessage(e) if e.type_ == a._DOCKLING_COMMAND => match e.data.as_data32()[0] {
                CMD_RELOAD => self.reload()?,
                CMD_QUIT => return Ok(false),
                _ => {}
            },
            Event::SelectionClear(e) if e.selection == self.selection => {
                eprintln!("dockling: replaced by another instance");
                return Ok(false);
            }
            Event::Error(e) if debug() => {
                eprintln!("dockling: X error {e:?}");
            }
            _ => {}
        }
        Ok(true)
    }

    fn event_loop(&mut self) -> Res<()> {
        use rustix::event::{PollFd, PollFlags, Timespec, poll};
        loop {
            self.conn.flush()?;
            while let Some(event) = self.conn.poll_for_event()? {
                if !self.handle(event)? {
                    return Ok(());
                }
            }
            // Reap finished launches so they do not linger as zombies.
            self.children.retain_mut(|c| !matches!(c.try_wait(), Ok(Some(_))));
            if let Some(at) = self.hide_at
                && Instant::now() >= at
            {
                self.hide_at = None;
                if self.menu.is_none() {
                    self.hover = None;
                    self.hide_tooltip()?;
                    self.set_hidden(true)?;
                }
                continue;
            }
            self.conn.flush()?;
            if let Some(event) = self.conn.poll_for_event()? {
                if !self.handle(event)? {
                    return Ok(());
                }
                continue;
            }
            let timeout = self.hide_at.map(|at| {
                let d = at.saturating_duration_since(Instant::now());
                Timespec { tv_sec: d.as_secs() as _, tv_nsec: d.subsec_nanos() as _ }
            });
            let mut fds = [PollFd::new(self.conn.stream(), PollFlags::IN)];
            match poll(&mut fds, timeout.as_ref()) {
                Ok(_) | Err(rustix::io::Errno::INTR) => {}
                Err(e) => return Err(e.into()),
            }
        }
    }
}

fn debug() -> bool {
    std::env::var_os("DOCKLING_DEBUG").is_some()
}

// -------------------------------------------------------------------- drawing helpers

fn paint(color: Color) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(color.skia());
    p.anti_alias = true;
    p
}

fn fill_rrect(pm: &mut Pixmap, r: R, radius: f32, color: Color) {
    let (x, y, w, h) = (r.x as f32, r.y as f32, r.w as f32, r.h as f32);
    let rad = radius.min(w / 2.0).min(h / 2.0);
    let path = if rad < 0.5 {
        Rect::from_xywh(x, y, w, h).map(PathBuilder::from_rect)
    } else {
        let k = rad * 0.447_715; // (1 - 0.552285) * rad: cubic circle approximation
        let mut pb = PathBuilder::new();
        pb.move_to(x + rad, y);
        pb.line_to(x + w - rad, y);
        pb.cubic_to(x + w - k, y, x + w, y + k, x + w, y + rad);
        pb.line_to(x + w, y + h - rad);
        pb.cubic_to(x + w, y + h - k, x + w - k, y + h, x + w - rad, y + h);
        pb.line_to(x + rad, y + h);
        pb.cubic_to(x + k, y + h, x, y + h - k, x, y + h - rad);
        pb.line_to(x, y + rad);
        pb.cubic_to(x, y + k, x + k, y, x + rad, y);
        pb.close();
        pb.finish()
    };
    if let Some(path) = path {
        pm.fill_path(&path, &paint(color), FillRule::Winding, Transform::identity(), None);
    }
}

/// The "Show Applications" 3x3 grid glyph.
fn draw_grid(pm: &mut Pixmap, r: R, color: Color) {
    let size = r.w as f32 * 0.5;
    let dot = size / 4.0;
    let step = (size - dot) / 2.0;
    let x0 = r.x as f32 + (r.w as f32 - size) / 2.0 + dot / 2.0;
    let y0 = r.y as f32 + (r.h as f32 - size) / 2.0 + dot / 2.0;
    for row in 0..3 {
        for col in 0..3 {
            let (cx, cy) = (x0 + col as f32 * step, y0 + row as f32 * step);
            if let Some(path) = PathBuilder::from_circle(cx, cy, dot / 2.0) {
                pm.fill_path(&path, &paint(color), FillRule::Winding, Transform::identity(), None);
            }
        }
    }
}
