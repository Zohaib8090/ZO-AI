//! The message box: several lines that grow as you type. Enter sends, Shift+Enter starts a new line.

use gtk4 as gtk;
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Clone)]
pub struct Prompt {
    root: gtk::Overlay,
    view: gtk::TextView,
    hint: gtk::Label,
    on_send: Rc<RefCell<Option<Rc<dyn Fn()>>>>,
}

impl Prompt {
    pub fn new(placeholder: &str) -> Prompt {
        let view = gtk::TextView::new();
        view.set_wrap_mode(gtk::WrapMode::WordChar);
        view.set_accepts_tab(false);
        view.add_css_class("composer-entry");
        view.set_top_margin(6);
        view.set_bottom_margin(10);
        view.set_left_margin(4);
        view.set_right_margin(4);

        let scroll = gtk::ScrolledWindow::new();
        scroll.set_child(Some(&view));
        scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
        scroll.set_propagate_natural_height(true);
        scroll.set_min_content_height(44);
        scroll.set_max_content_height(220);
        scroll.set_hexpand(true);

        let hint = gtk::Label::new(Some(placeholder));
        hint.add_css_class("composer-hint");
        hint.set_halign(gtk::Align::Start);
        hint.set_valign(gtk::Align::Start);
        hint.set_can_target(false);
        hint.set_margin_top(6);
        hint.set_margin_start(4);

        let root = gtk::Overlay::new();
        root.set_child(Some(&scroll));
        root.add_overlay(&hint);
        root.set_hexpand(true);

        let on_send: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));
        {
            let hint = hint.clone();
            view.buffer().connect_changed(move |b| hint.set_visible(b.char_count() == 0));
        }
        {
            let on_send = on_send.clone();
            let keys = gtk::EventControllerKey::new();
            keys.connect_key_pressed(move |_, key, _, state| {
                let enter = key == gtk::gdk::Key::Return || key == gtk::gdk::Key::KP_Enter;
                if enter && !state.contains(gtk::gdk::ModifierType::SHIFT_MASK) {
                    let f = on_send.borrow().clone();
                    if let Some(f) = f {
                        f();
                    }
                    return gtk::glib::Propagation::Stop;
                }
                gtk::glib::Propagation::Proceed
            });
            view.add_controller(keys);
        }
        Prompt { root, view, hint, on_send }
    }

    pub fn widget(&self) -> &gtk::Overlay {
        &self.root
    }

    pub fn text(&self) -> String {
        let b = self.view.buffer();
        b.text(&b.start_iter(), &b.end_iter(), false).to_string()
    }

    pub fn set_text(&self, text: &str) {
        self.view.buffer().set_text(text);
    }

    /// Puts the cursor at the end.
    pub fn move_to_end(&self) {
        let b = self.view.buffer();
        b.place_cursor(&b.end_iter());
    }

    pub fn set_placeholder(&self, text: &str) {
        self.hint.set_text(text);
    }

    pub fn grab_focus(&self) {
        self.view.grab_focus();
    }

    pub fn set_sensitive(&self, on: bool) {
        self.view.set_sensitive(on);
    }

    /// Called when Enter is pressed (not Shift+Enter).
    pub fn connect_send(&self, f: impl Fn() + 'static) {
        *self.on_send.borrow_mut() = Some(Rc::new(f));
    }

    pub fn window(&self) -> Option<gtk::Window> {
        self.root.root().and_then(|r| r.downcast::<gtk::Window>().ok())
    }
}
