//! Preset bar for the plugin header: save, name, list, previous / next.
//!
//! A preset is a set of parameter values. Built into the plugin is only the
//! starting point ("Default"); all other presets, also the ones that ship with a
//! release, are JSON files (a nice-plug `PluginState` without
//! fields) in `Documents/Anselic/Sacria/<plugin>/Presets`; a folder in there is a
//! category in the list (one level). A preset is found by its name, so names are
//! unique across categories. The name of the loaded preset lives in
//! a persisted field of the plugin's params, so it comes back with the project.

use crate::{Canvas, DIM, FAINT, INK, MEDIUM, REGULAR, WHITE};
use egui::{Align2, Color32, Key, Rect, Response, Sense, Shape, Stroke, Ui, Vec2};
use nice_plug::context::gui::GuiContext;
use nice_plug::plugin::ParamValue;
use nice_plug::prelude::{Param, ParamPtr, Params, PluginState};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

/// Name of the preset every plugin starts with: all parameters at their defaults.
pub const DEFAULT_NAME: &str = "Default";

/// Bar height (design units).
pub const BAR_H: f32 = 26.0;
const BTN_W: f32 = 30.0;
const ROW_H: f32 = 20.0;
const LIST_W: f32 = 320.0;
const LIST_ROWS: f32 = 14.0;
const NAME_MAX: usize = 40;
const SOFT: Color32 = Color32::from_rgb(0x9a, 0x9a, 0x96);

type Values = BTreeMap<String, ParamValue>;

/// A built-in preset. `params` is JSON: a whole preset file as the plugin saves
/// it, or only a map of the parameters that differ from the defaults,
/// `{"lim_drive": {"f32": 6.0}}`. `category` is its group in the list ("" = none).
pub struct Factory {
    pub name: &'static str,
    pub category: &'static str,
    pub params: &'static str,
}

struct Preset {
    name: String,
    category: String,
    params: Values,
    user: bool,
    /// User presets: the file.
    path: Option<PathBuf>,
}

/// A preset's values from JSON: a whole preset file, or only its map of parameters.
fn parse(json: &[u8]) -> Option<Values> {
    serde_json::from_slice::<PluginState>(json).map(|s| s.params).or_else(|_| serde_json::from_slice::<Values>(json)).ok()
}

enum Row {
    Label(String),
    Preset(usize),
    Folder,
}

pub struct PresetBar {
    plugin: &'static str,
    /// Keeps the parameters behind `ptrs` alive.
    _live: Arc<dyn Params>,
    ptrs: Vec<(String, ParamPtr)>,
    defaults: Values,
    name: Arc<RwLock<String>>,
    /// Parameters a preset never touches (they keep their value on load).
    skip: &'static [&'static str],
    presets: Vec<Preset>,
    open: bool,
    /// Save: the name being typed, and whether the field still has to grab the keyboard.
    naming: Option<String>,
    grab_focus: bool,
    /// Short message in the bar and the time it goes away.
    note: Option<(&'static str, f64)>,
    /// Debug builds only: preset to load on the first frame (see `new`).
    debug_load: Option<String>,
}

/// Same as nice-plug's own state serialization.
///
/// # Safety
///
/// The parameter behind `ptr` must be alive.
unsafe fn read(ptr: ParamPtr) -> ParamValue {
    unsafe {
        match ptr {
            ParamPtr::FloatParam(p) => ParamValue::F32((*p).unmodulated_plain_value()),
            ParamPtr::IntParam(p) => ParamValue::I32((*p).unmodulated_plain_value()),
            ParamPtr::BoolParam(p) => ParamValue::Bool((*p).unmodulated_plain_value()),
            ParamPtr::EnumParam(p) => match (*p).unmodulated_plain_id() {
                Some(id) => ParamValue::String(id.to_owned()),
                None => ParamValue::I32((*p).unmodulated_plain_value()),
            },
        }
    }
}

fn values(params: &dyn Params) -> Values {
    // the pointers are only used while `params` is borrowed
    params.param_map().into_iter().map(|(id, ptr, _)| (id, unsafe { read(ptr) })).collect()
}

fn same(a: &ParamValue, b: &ParamValue) -> bool {
    match (a, b) {
        (ParamValue::F32(a), ParamValue::F32(b)) => (a - b).abs() <= 1e-4 * (1.0 + a.abs()),
        (ParamValue::I32(a), ParamValue::I32(b)) => a == b,
        (ParamValue::Bool(a), ParamValue::Bool(b)) => a == b,
        (ParamValue::String(a), ParamValue::String(b)) => a == b,
        _ => false,
    }
}

fn same_kind(a: &ParamValue, b: &ParamValue) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b)
}

/// A preset name that is safe as a file name. Empty if nothing usable is left.
fn clean_name(name: &str) -> String {
    let s: String = name.chars().filter(|c| !c.is_control() && !"<>:\"/\\|?*".contains(*c)).collect();
    s.trim().trim_end_matches('.').chars().take(NAME_MAX).collect::<String>().trim().to_owned()
}

/// Checks the factory presets of a plugin: valid JSON, known parameter ids,
/// right value types, no double names. For the plugin's tests.
pub fn check_factory(params: &dyn Params, factory: &[Factory]) -> Result<(), String> {
    let defaults = values(params);
    for (i, f) in factory.iter().enumerate() {
        if clean_name(f.name) != f.name {
            return Err(format!("{}: not a clean name", f.name));
        }
        if factory[..i].iter().any(|g| g.name.eq_ignore_ascii_case(f.name)) {
            return Err(format!("{}: name used twice", f.name));
        }
        if clean_name(f.category) != f.category {
            return Err(format!("{}: category is not a clean name", f.name));
        }
        let map = parse(f.params.as_bytes()).ok_or_else(|| format!("{}: not a preset", f.name))?;
        for (id, v) in &map {
            match defaults.get(id) {
                None => return Err(format!("{}: unknown parameter {id}", f.name)),
                Some(d) if !same_kind(d, v) => return Err(format!("{}: wrong value type for {id}", f.name)),
                _ => {}
            }
        }
    }
    Ok(())
}

impl PresetBar {
    /// `live`: the plugin's parameters. `defaults`: a fresh instance of the same
    /// params struct. `name`: the persisted field that holds the preset name.
    pub fn new(
        plugin: &'static str,
        live: Arc<dyn Params>,
        defaults: &dyn Params,
        name: Arc<RwLock<String>>,
        factory: &[Factory],
        skip: &'static [&'static str],
    ) -> Self {
        let ptrs = live.param_map().into_iter().map(|(id, ptr, _)| (id, ptr)).collect();
        let mut presets: Vec<Preset> = factory
            .iter()
            .map(|f| Preset { name: f.name.to_owned(), category: f.category.to_owned(), params: parse(f.params.as_bytes()).unwrap_or_default(), user: false, path: None })
            .collect();
        // the list shows a category as one block: no category first, the rest by name
        presets.sort_by_key(|p| p.category.to_lowercase());
        // debug builds only, for screenshots: SACRIA_UI_PRESET=list opens the list,
        // =save opens the name field, =load:<name> loads that preset on the first frame
        let debug = if cfg!(debug_assertions) { std::env::var("SACRIA_UI_PRESET").unwrap_or_default() } else { String::new() };
        let mut bar = Self {
            plugin,
            _live: live,
            ptrs,
            defaults: values(defaults),
            name,
            skip,
            presets,
            open: debug == "list",
            naming: (debug == "save").then(|| "My preset".to_owned()),
            grab_focus: debug == "save",
            note: None,
            debug_load: debug.strip_prefix("load:").map(str::to_owned),
        };
        bar.rescan();
        bar
    }

    /// Where the user presets live. None if the system has no documents folder.
    pub fn folder(&self) -> Option<PathBuf> {
        Some(dirs::document_dir()?.join("Anselic").join("Sacria").join(self.plugin).join("Presets"))
    }

    /// Reads the user presets from disk again.
    fn rescan(&mut self) {
        self.presets.retain(|p| !p.user);
        let Some(root) = self.folder() else {
            return;
        };
        // the folder itself, then its sub-folders as categories
        let mut dirs = vec![(String::new(), root.clone())];
        for dir in std::fs::read_dir(&root).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
            if let Some(name) = dir.file_name().and_then(|s| s.to_str()) {
                dirs.push((name.to_owned(), dir.clone()));
            }
        }
        let mut user: Vec<Preset> = Vec::new();
        for (category, dir) in dirs {
            for path in std::fs::read_dir(&dir).into_iter().flatten().flatten().map(|e| e.path()) {
                if !path.is_file() || !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("json")) {
                    continue;
                }
                let Some(name) = path.file_stem().and_then(|s| s.to_str()).map(str::to_owned) else {
                    continue;
                };
                // names are unique: a factory preset or an earlier file of the same name wins
                if self.presets.iter().chain(&user).any(|p| p.name.eq_ignore_ascii_case(&name)) {
                    continue;
                }
                if let Some(params) = std::fs::read(&path).ok().and_then(|b| parse(&b)) {
                    user.push(Preset { name, category: category.clone(), params, user: true, path: Some(path) });
                }
            }
        }
        user.sort_by_key(|p| (p.category.to_lowercase(), p.name.to_lowercase()));
        self.presets.extend(user);
    }

    fn current_name(&self) -> String {
        self.name.read().map(|n| n.clone()).unwrap_or_default()
    }

    /// The value preset `p` gives parameter `id`.
    fn target<'a>(&'a self, p: &'a Preset, id: &str) -> Option<&'a ParamValue> {
        let d = self.defaults.get(id)?;
        Some(p.params.get(id).filter(|v| same_kind(v, d)).unwrap_or(d))
    }

    /// Did a knob move since preset `i` was loaded?
    fn differs(&self, i: usize) -> bool {
        let p = &self.presets[i];
        self.ptrs.iter().filter(|(id, _)| !self.skip.contains(&id.as_str())).any(|(id, ptr)| {
            // `_live` keeps the parameter alive
            self.target(p, id).is_some_and(|t| !same(&unsafe { read(*ptr) }, t))
        })
    }

    fn load(&mut self, i: usize, gui: &GuiContext) {
        let p = &self.presets[i];
        if let Ok(mut n) = self.name.write() {
            *n = p.name.clone();
        }
        // the state carries the new name along in its fields
        let mut state = gui.get_state();
        for (id, _) in self.ptrs.iter().filter(|(id, _)| !self.skip.contains(&id.as_str())) {
            if let Some(v) = self.target(p, id) {
                state.params.insert(id.clone(), v.clone());
            }
        }
        gui.set_state(state);
    }

    /// Previous (-1) or next (+1) preset, around the ends.
    fn step(&mut self, dir: isize, gui: &GuiContext) {
        let n = self.presets.len() as isize;
        if n == 0 {
            return;
        }
        let name = self.current_name();
        let i = match self.presets.iter().position(|p| p.name == name) {
            Some(i) => (i as isize + dir).rem_euclid(n),
            None => 0,
        };
        self.load(i as usize, gui);
    }

    /// Writes the current settings as a user preset. Err = the message for the bar.
    fn save(&mut self, typed: &str, gui: &GuiContext) -> Result<(), &'static str> {
        let name = clean_name(typed);
        if name.is_empty() {
            return Err("TYPE A NAME");
        }
        if self.presets.iter().any(|p| !p.user && p.name.eq_ignore_ascii_case(&name)) {
            return Err("THAT NAME IS BUILT IN, PICK ANOTHER");
        }
        // a preset that is already there keeps its file (and so its category)
        let old = self.presets.iter().find(|p| p.name.eq_ignore_ascii_case(&name)).and_then(|p| p.path.clone());
        let file = match old {
            Some(f) => f,
            None => self.folder().ok_or("NO DOCUMENTS FOLDER")?.join(format!("{name}.json")),
        };
        let mut state = gui.get_state();
        state.fields.clear();
        state.params.retain(|id, _| !self.skip.contains(&id.as_str()));
        let json = serde_json::to_vec_pretty(&state).map_err(|_| "SAVE FAILED")?;
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(|_| "SAVE FAILED")?;
        }
        std::fs::write(&file, json).map_err(|_| "SAVE FAILED")?;
        if let Ok(mut n) = self.name.write() {
            *n = name;
        }
        self.rescan();
        Ok(())
    }

    fn open_folder(&self) {
        let Some(dir) = self.folder() else {
            return;
        };
        let _ = std::fs::create_dir_all(&dir);
        let tool = if cfg!(target_os = "windows") {
            "explorer"
        } else if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        let _ = std::process::Command::new(tool).arg(dir).spawn();
    }

    /// Draws the bar from `x0` to `x1`, centred on `cy` (design units), on the
    /// paper background. Returns the hover response of the whole bar.
    pub fn show(&mut self, ui: &mut Ui, c: &Canvas, x0: f32, x1: f32, cy: f32, gui: &GuiContext) -> Response {
        let (y0, y1) = (cy - BAR_H / 2.0, cy + BAR_H / 2.0);
        let id = ui.id().with("preset-bar");
        let now = ui.input(|i| i.time);
        let bar = c.r(x0, y0, x1, y1);
        ui.painter().rect_filled(bar, c.l(6.0), INK);
        let mut resp = ui.interact(bar, id, Sense::hover());

        if let Some(i) = self.debug_load.take().and_then(|n| self.presets.iter().position(|p| p.name == n)) {
            self.load(i, gui);
        }
        let name = self.current_name();
        let index = self.presets.iter().position(|p| p.name == name);
        let tint = |r: &Response| if r.hovered() { WHITE } else { SOFT };

        // save: a disk
        let save = ui.interact(c.r(x0, y0, x0 + BTN_W, y1), id.with("save"), Sense::click());
        {
            let (x, col) = (x0 + BTN_W / 2.0 + 1.0, tint(&save));
            c.line(ui, &[(x - 6.0, cy - 6.0), (x + 3.5, cy - 6.0), (x + 6.0, cy - 3.5), (x + 6.0, cy + 6.0), (x - 6.0, cy + 6.0), (x - 6.0, cy - 6.0)], 1.2, col);
            c.line(ui, &[(x - 3.0, cy - 6.0), (x - 3.0, cy - 2.5), (x + 2.5, cy - 2.5), (x + 2.5, cy - 6.0)], 1.2, col);
            c.line(ui, &[(x - 3.5, cy + 6.0), (x - 3.5, cy + 1.0), (x + 3.5, cy + 1.0), (x + 3.5, cy + 6.0)], 1.2, col);
        }
        if save.clicked() {
            self.naming = Some(name.clone());
            self.grab_focus = true;
            self.open = false;
        }
        c.line(ui, &[(x0 + BTN_W + 2.0, y0 + 6.0), (x0 + BTN_W + 2.0, y1 - 6.0)], 1.0, FAINT);

        // previous / next
        let ax = x1 - 2.0 * BTN_W + 4.0;
        c.line(ui, &[(ax - 2.0, y0 + 6.0), (ax - 2.0, y1 - 6.0)], 1.0, FAINT);
        for (k, dir) in [-1isize, 1].into_iter().enumerate() {
            let bx = ax + (BTN_W - 2.0) * k as f32;
            let r = ui.interact(c.r(bx, y0, bx + BTN_W - 2.0, y1), id.with(("step", k)), Sense::click());
            let (mx, d) = (bx + (BTN_W - 2.0) / 2.0, 2.5 * dir as f32);
            c.line(ui, &[(mx - d, cy - 5.0), (mx + d, cy), (mx - d, cy + 5.0)], 1.4, tint(&r));
            if r.clicked() {
                self.naming = None;
                self.open = false;
                self.step(dir, gui);
            }
            resp = resp.union(r);
        }

        let (nx0, nx1) = (x0 + BTN_W + 4.0, ax - 4.0);
        // `now` starts again at 0 each time the editor window opens: a note that is due later than any note lasts is an old one
        let note = self.note.filter(|(_, until)| now < *until && *until - now <= 3.0).map(|(text, _)| text);
        let mut toggle_rect = Rect::NOTHING;
        if let Some(mut text) = self.naming.take() {
            // save: type the name here
            let hint = note.unwrap_or("ENTER = SAVE, ESC = CANCEL");
            c.text(ui, nx1 - 6.0, cy, Align2::RIGHT_CENTER, hint, 8.0, REGULAR, DIM);
            let field = c.r(nx0 + 8.0, y0 + 3.0, nx1 - 150.0, y1 - 3.0);
            let edit = ui
                .scope(|ui| {
                    let v = ui.visuals_mut();
                    v.text_cursor.stroke.color = WHITE;
                    v.selection.bg_fill = DIM;
                    v.selection.stroke.color = WHITE;
                    ui.put(
                        field,
                        egui::TextEdit::singleline(&mut text)
                            .frame(egui::Frame::NONE)
                            .font(c.font(11.0, MEDIUM))
                            .text_color(WHITE)
                            .char_limit(NAME_MAX)
                            .vertical_align(egui::Align::Center)
                            .margin(Vec2::ZERO),
                    )
                })
                .inner;
            if std::mem::take(&mut self.grab_focus) {
                edit.request_focus();
            }
            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            let cancel = ui.input(|i| i.key_pressed(Key::Escape)) || (edit.lost_focus() && !enter);
            if enter {
                match self.save(&text, gui) {
                    Ok(()) => self.note = Some(("SAVED", now + 2.0)),
                    Err(why) => {
                        // stay in the field so the name can be fixed
                        self.note = Some((why, now + 3.0));
                        self.naming = Some(text);
                        self.grab_focus = true;
                    }
                }
            } else if !cancel {
                self.naming = Some(text);
            }
            resp = resp.union(edit);
        } else {
            // name: click = list
            toggle_rect = c.r(nx0, y0, nx1, y1);
            let toggle = ui.interact(toggle_rect, id.with("name"), Sense::click());
            if toggle.clicked() {
                self.open = !self.open;
                if self.open {
                    self.rescan();
                }
            }
            let dirty = index.is_none_or(|i| self.differs(i));
            let shown = if dirty { format!("{name} *") } else { name.clone() };
            c.text(ui, nx0 + 8.0, cy, Align2::LEFT_CENTER, &shown, 11.0, MEDIUM, WHITE);
            let col = if toggle.hovered() || self.open { WHITE } else { SOFT };
            let (tx, s) = (nx1 - 12.0, if self.open { -1.0 } else { 1.0 });
            ui.painter().add(Shape::convex_polygon(
                vec![c.p(tx - 4.0, cy - 2.5 * s), c.p(tx + 4.0, cy - 2.5 * s), c.p(tx, cy + 3.0 * s)],
                col,
                Stroke::NONE,
            ));
            let count = match index {
                Some(i) => format!("{:02} / {:02}", i + 1, self.presets.len()),
                None => format!("-- / {:02}", self.presets.len()),
            };
            c.text(ui, tx - 14.0, cy, Align2::RIGHT_CENTER, note.unwrap_or(&count), 8.0, REGULAR, DIM);
            resp = resp.union(toggle);
        }
        if note.is_some() {
            ui.ctx().request_repaint();
        }

        if self.open {
            self.list(ui, c, x0, y1 + 4.0, (x1 - x0).min(LIST_W), index, toggle_rect, gui);
        }
        resp.union(save)
    }

    /// The drop-down list, in its own layer above the editor.
    #[allow(clippy::too_many_arguments)]
    fn list(&mut self, ui: &mut Ui, c: &Canvas, x: f32, y: f32, w: f32, index: Option<usize>, toggle_rect: Rect, gui: &GuiContext) {
        // built-in first, then the files: no category first, then one block per category
        let mut rows = Vec::new();
        let mut category = "";
        for (i, p) in self.presets.iter().enumerate() {
            if p.category != category {
                category = &p.category;
                rows.push(Row::Label(category.to_uppercase()));
            }
            rows.push(Row::Preset(i));
        }
        if !self.presets.iter().any(|p| p.user) {
            rows.push(Row::Label("NO SAVED PRESETS YET. THE DISK ICON SAVES ONE.".to_owned()));
        }
        rows.push(Row::Folder);

        let mut picked: Option<&Row> = None;
        let pad = c.l(6.0);
        let area = egui::Area::new(ui.id().with("preset-list")).order(egui::Order::Foreground).fixed_pos(c.p(x, y)).show(ui.ctx(), |ui| {
            ui.style_mut().visuals = egui::Visuals::dark();
            egui::Frame::new().fill(INK).stroke(Stroke::new(c.l(1.0), DIM)).corner_radius(c.l(6.0)).inner_margin(pad).show(ui, |ui| {
                ui.set_width(c.l(w) - 2.0 * pad);
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                egui::ScrollArea::vertical().max_height(c.l(ROW_H * LIST_ROWS)).show(ui, |ui| {
                    for row in &rows {
                        let sense = if matches!(row, Row::Label(_)) { Sense::hover() } else { Sense::click() };
                        let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), c.l(ROW_H)), sense);
                        let hot = resp.hovered() && !matches!(row, Row::Label(_));
                        if hot {
                            ui.painter().rect_filled(r, c.l(3.0), FAINT);
                        }
                        let at = |dx: f32| egui::pos2(r.left() + c.l(dx), r.center().y);
                        match row {
                            Row::Label(text) => {
                                ui.painter().text(at(6.0), Align2::LEFT_CENTER, text.as_str(), c.font(8.0, REGULAR), DIM);
                            }
                            Row::Preset(i) => {
                                let on = index == Some(*i);
                                if on {
                                    ui.painter().rect_filled(Rect::from_center_size(at(10.0), Vec2::splat(c.l(5.0))), 0.0, WHITE);
                                }
                                let col = if on || hot { WHITE } else { SOFT };
                                ui.painter().text(at(20.0), Align2::LEFT_CENTER, &self.presets[*i].name, c.font(10.0, MEDIUM), col);
                            }
                            Row::Folder => {
                                ui.painter().line_segment([r.left_top(), r.right_top()], Stroke::new(c.l(1.0), FAINT));
                                let col = if hot { WHITE } else { DIM };
                                ui.painter().text(at(6.0), Align2::LEFT_CENTER, "OPEN PRESET FOLDER", c.font(8.0, REGULAR), col);
                            }
                        }
                        if resp.clicked() {
                            picked = Some(row);
                        }
                    }
                });
            });
        });
        match picked {
            Some(Row::Preset(i)) => {
                self.open = false;
                self.load(*i, gui);
            }
            Some(Row::Folder) => {
                self.open = false;
                self.open_folder();
            }
            _ => {}
        }
        // a click anywhere else closes the list
        let outside = ui.input(|i| {
            i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !area.response.rect.contains(p) && !toggle_rect.contains(p))
        });
        if outside || ui.input(|i| i.key_pressed(Key::Escape)) {
            self.open = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_preset_reads_from_a_whole_file_or_a_bare_map() {
        let file = br#"{"version": "1", "params": {"a": {"f32": 2.0}}, "fields": {}}"#;
        assert!(same(&parse(file).unwrap()["a"], &ParamValue::F32(2.0)));
        assert!(same(&parse(br#"{"a": {"f32": 2.0}}"#).unwrap()["a"], &ParamValue::F32(2.0)));
        assert!(parse(b"{}").unwrap().is_empty());
        assert!(parse(b"nope").is_none());
    }

    #[test]
    fn names_are_safe_file_names() {
        assert_eq!(clean_name("  Loud / Wide?  "), "Loud  Wide");
        assert_eq!(clean_name("a\\b:c*d"), "abcd");
        assert_eq!(clean_name("dots..."), "dots");
        assert_eq!(clean_name(" \t "), "");
        assert_eq!(clean_name(&"x".repeat(100)).len(), NAME_MAX);
    }

    #[test]
    fn values_compare_with_a_small_tolerance() {
        assert!(same(&ParamValue::F32(1.0), &ParamValue::F32(1.00001)));
        assert!(!same(&ParamValue::F32(1.0), &ParamValue::F32(1.01)));
        assert!(same(&ParamValue::String("soft".into()), &ParamValue::String("soft".into())));
        assert!(!same(&ParamValue::Bool(true), &ParamValue::I32(1)));
    }

    #[test]
    fn preset_values_read_like_a_state_file() {
        let map: Values = serde_json::from_str(r#"{"a": {"f32": 6.0}, "b": {"string": "soft"}, "c": {"bool": true}}"#).unwrap();
        assert!(same(&map["a"], &ParamValue::F32(6.0)));
        assert!(same(&map["b"], &ParamValue::String("soft".into())));
        assert!(same(&map["c"], &ParamValue::Bool(true)));
    }
}
