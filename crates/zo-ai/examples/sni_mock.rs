//! A tiny StatusNotifierWatcher for testing the tray icon without a desktop: it owns org.kde.StatusNotifierWatcher on the session
//! bus, writes every item that registers (one per line) to the file given as the first argument, and runs until killed.
//! Run it inside a private bus: dbus-run-session -- sni_mock /tmp/x/items.txt

use gtk4::gio;
use gtk4::glib;
use gtk4::glib::variant::ToVariant;

const XML: &str = r#"<node><interface name="org.kde.StatusNotifierWatcher">
  <method name="RegisterStatusNotifierItem"><arg type="s" name="service" direction="in"/></method>
  <method name="RegisterStatusNotifierHost"><arg type="s" name="service" direction="in"/></method>
  <property name="RegisteredStatusNotifierItems" type="as" access="read"/>
  <property name="IsStatusNotifierHostRegistered" type="b" access="read"/>
  <property name="ProtocolVersion" type="i" access="read"/>
  <signal name="StatusNotifierItemRegistered"><arg type="s"/></signal>
</interface></node>"#;

fn main() {
    let out = std::env::args().nth(1).expect("file for the registered items");
    let ctx = glib::MainContext::default();
    let main_loop = glib::MainLoop::new(Some(&ctx), false);
    let conn = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).expect("session bus");
    let info = gio::DBusNodeInfo::for_xml(XML).unwrap();
    let iface = info.lookup_interface("org.kde.StatusNotifierWatcher").unwrap();
    let c2 = conn.clone();
    conn.register_object("/StatusNotifierWatcher", &iface)
        .method_call(move |_, sender, _, _, method, params, inv| {
            if method == "RegisterStatusNotifierItem" {
                let svc: String = params.child_value(0).get().unwrap_or_default();
                let line = format!("{} {}\n", sender.unwrap_or(""), svc);
                let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&out).unwrap();
                std::io::Write::write_all(&mut f, line.as_bytes()).unwrap();
                let _ = c2.emit_signal(None, "/StatusNotifierWatcher", "org.kde.StatusNotifierWatcher", "StatusNotifierItemRegistered", Some(&(svc,).to_variant()));
            }
            inv.return_value(None);
        })
        .property(|_, _, _, _, name| match name {
            "IsStatusNotifierHostRegistered" => true.to_variant(),
            "ProtocolVersion" => 0i32.to_variant(),
            _ => Vec::<String>::new().to_variant(),
        })
        .build()
        .unwrap();
    let _id = gio::bus_own_name_on_connection(&conn, "org.kde.StatusNotifierWatcher", gio::BusNameOwnerFlags::NONE, |_, _| eprintln!("watcher up"), |_, _| {});
    main_loop.run();
}
