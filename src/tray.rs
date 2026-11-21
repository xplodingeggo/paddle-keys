use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, TrayIconBuilder};

use crate::autostart;

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
pub fn run(paused: Arc<AtomicBool>, stop: Arc<AtomicBool>, poller: JoinHandle<()>, config: Option<PathBuf>) {
    let event_loop = EventLoopBuilder::new().build();
    let pause_i = CheckMenuItem::new("Paused", true, false, None);
    let auto_i = CheckMenuItem::new("Start with Windows", true, autostart::is_enabled(), None);
    let cfg_i = MenuItem::new("Open config", true, None);
    let quit_i = MenuItem::new("Quit", true, None);
    let menu = Menu::new();
    menu.append_items(&[&pause_i, &auto_i, &cfg_i, &quit_i]).expect("menu");
    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("paddle-keys")
        .with_icon(icon())
        .build()
        .expect("tray icon");
    let mut poller = Some(poller);
    let rx = MenuEvent::receiver();

    event_loop.run(move |event, _, flow| {
        *flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(100));
        if !matches!(event, Event::NewEvents(StartCause::ResumeTimeReached { .. } | StartCause::Init)) {
            return;
        }
        while let Ok(ev) = rx.try_recv() {
            if ev.id == pause_i.id() {
                paused.store(pause_i.is_checked(), Ordering::Relaxed);
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
                *flow = ControlFlow::Exit;
            }
        }
    });
}
