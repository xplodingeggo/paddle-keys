use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::autostart;
use crate::steam::{Mode, State};

fn icon() -> Icon {
    let mut px = Vec::with_capacity(16 * 16 * 4);
    for y in 0..16i32 {
        for x in 0..16i32 {
            let inside = (x - 8) * (x - 8) + (y - 8) * (y - 8) <= 49;
            px.extend_from_slice(if inside { &[0x2E, 0xA0, 0xF0, 0xFF] } else { &[0, 0, 0, 0] });
        }
    }
    Icon::from_rgba(px, 16, 16).expect("valid icon")
}

/// Runs the tray loop on the calling thread; joins `poller` on Quit, then exits the process.
pub fn run(paused: Arc<AtomicBool>, stop: Arc<AtomicBool>, poller: JoinHandle<()>, config: Option<PathBuf>, steam: Arc<State>) {
    let event_loop = EventLoopBuilder::new().build();
    let pause_i = CheckMenuItem::new("Paused", true, false, None);
    let auto_i = CheckMenuItem::new("Start with Windows", true, autostart::is_enabled(), None);
    let cfg_i = MenuItem::new("Open config (restart to apply)", true, None);
    let quit_i = MenuItem::new("Quit", true, None);

    let mode = Mode::from_u8(steam.mode.load(Ordering::Relaxed));
    let elite_i = CheckMenuItem::new("Enabled (loads into Steam)", true, steam.enabled.load(Ordering::Relaxed), None);
    let modes = [
        (CheckMenuItem::new("Paddles: Steam when active, else keys", true, mode == Mode::Auto, None), Mode::Auto),
        (CheckMenuItem::new("Paddles: keys only", true, mode == Mode::Keys, None), Mode::Keys),
        (CheckMenuItem::new("Paddles: Steam only", true, mode == Mode::Steam, None), Mode::Steam),
        (CheckMenuItem::new("Paddles: keys and Steam at once", true, mode == Mode::Both, None), Mode::Both),
    ];
    let status_i = MenuItem::new(steam.status().text(), false, None);
    let elite = Submenu::new("Steam Elite mode", true);
    elite.append(&elite_i).expect("menu");
    elite.append(&PredefinedMenuItem::separator()).expect("menu");
    for (item, _) in &modes {
        elite.append(item).expect("menu");
    }
    elite.append(&PredefinedMenuItem::separator()).expect("menu");
    elite.append(&status_i).expect("menu");

    let menu = Menu::new();
    menu.append_items(&[&pause_i, &elite, &auto_i, &cfg_i, &quit_i]).expect("menu");
    let mut tray: Option<TrayIcon> = Some(TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("paddle-keys")
        .with_icon(icon())
        .build()
        .expect("tray icon"));
    let mut poller = Some(poller);
    let mut shown = steam.status();
    let rx = MenuEvent::receiver();

    event_loop.run(move |event, _, flow| {
        if !matches!(event, Event::NewEvents(StartCause::ResumeTimeReached { .. } | StartCause::Init)) {
            return;
        }
        *flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(100));
        if steam.status() != shown {
            shown = steam.status();
            status_i.set_text(shown.text());
        }
        while let Ok(ev) = rx.try_recv() {
            if ev.id == pause_i.id() {
                paused.store(pause_i.is_checked(), Ordering::Relaxed);
            } else if ev.id == elite_i.id() {
                steam.enabled.store(elite_i.is_checked(), Ordering::Relaxed);
                steam.save();
            } else if let Some((_, m)) = modes.iter().find(|(item, _)| ev.id == item.id()) {
                // radio group: exactly one mode stays checked
                for (item, other) in &modes {
                    item.set_checked(other == m);
                }
                steam.mode.store(*m as u8, Ordering::Relaxed);
                steam.save();
            } else if ev.id == auto_i.id() {
                let _ = autostart::set(auto_i.is_checked());
                auto_i.set_checked(autostart::is_enabled());
            } else if ev.id == cfg_i.id() {
                if let Some(p) = &config {
                    let _ = std::process::Command::new("notepad.exe").arg(p).spawn();
                }
            } else if ev.id == quit_i.id() {
                stop.store(true, Ordering::Relaxed);
                if let Some(h) = poller.take() {
                    let _ = h.join();
                }
                steam.shutdown();
                drop(tray.take()); // process::exit skips destructors; drop removes the icon
                *flow = ControlFlow::Exit;
            }
        }
    });
}
