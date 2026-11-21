use crate::config::{Bindings, KeySpec};
use crate::decode::{Paddle, Paddles};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
    VIRTUAL_KEY,
};

type Sender = Box<dyn FnMut(&KeySpec, bool)>;
const ALL: [Paddle; 4] = [Paddle::L4, Paddle::R4, Paddle::PL, Paddle::PR];

pub fn edges(prev: Paddles, next: Paddles) -> Vec<(Paddle, bool)> {
    ALL.into_iter()
        .filter(|&p| prev.get(p) != next.get(p))
        .map(|p| (p, next.get(p)))
        .collect()
}

pub struct Output {
    b: Bindings,
    held: Paddles,
    sender: Sender,
}

impl Output {
    pub fn new(b: Bindings) -> Output {
        Self::with_sender(b, send)
    }
    pub fn with_sender(b: Bindings, f: impl FnMut(&KeySpec, bool) + 'static) -> Output {
        Output { b, held: Paddles::default(), sender: Box::new(f) }
    }
    pub fn apply(&mut self, next: Paddles) {
        for (p, down) in edges(self.held, next) {
            (self.sender)(self.b.get(p), down);
        }
        self.held = next;
    }
    pub fn release_all(&mut self) {
        self.apply(Paddles::default());
    }
}

// Verified manually (real SendInput is not exercised in unit tests).
fn send(spec: &KeySpec, down: bool) {
    let ev = |vk: u16, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                ..Default::default()
            },
        },
    };
    let inputs: Vec<INPUT> = if down {
        spec.mods.iter().map(|&m| ev(m, false)).chain([ev(spec.key, false)]).collect()
    } else {
        [ev(spec.key, true)].into_iter().chain(spec.mods.iter().rev().map(|&m| ev(m, true))).collect()
    };
    unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse_keyspec;
    use std::{cell::RefCell, rc::Rc};

    fn p(l4: bool, r4: bool, pl: bool, pr: bool) -> Paddles {
        Paddles { l4, r4, pl, pr }
    }

    #[test]
    fn edge_cases() {
        let none = Paddles::default();
        let pl = p(false, false, true, false);
        let all = p(true, true, true, true);
        assert_eq!(edges(none, pl), vec![(Paddle::PL, true)]);
        assert_eq!(edges(pl, none), vec![(Paddle::PL, false)]);
        assert_eq!(edges(pl, pl), vec![]);
        assert_eq!(edges(none, all).len(), 4);
        assert!(edges(none, all).iter().all(|e| e.1));
        let r = edges(all, p(false, true, false, false));
        assert_eq!(r.len(), 3);
        assert!(r.iter().all(|e| !e.1));
    }

    type Log = Rc<RefCell<Vec<(KeySpec, bool)>>>;
    fn out(b: Bindings) -> (Output, Log) {
        let log: Log = Default::default();
        let l = log.clone();
        (Output::with_sender(b, move |s, d| l.borrow_mut().push((s.clone(), d))), log)
    }

    fn ctrl_f15_bindings() -> Bindings {
        let path = std::env::temp_dir().join("paddle-keys-keys-test.toml");
        std::fs::write(&path, "[keys]
PL = \"Ctrl+F15\"
").unwrap();
        crate::config::load(&path)
    }

    #[test]
    fn press_sends_down_for_bound_spec() {
        let (mut o, log) = out(ctrl_f15_bindings());
        o.apply(p(false, false, true, false));
        let want = parse_keyspec("Ctrl+F15").unwrap();
        assert_eq!(*log.borrow(), vec![(want, true)]);
    }

    #[test]
    fn release_all_sends_ups_once() {
        let (mut o, log) = out(Bindings::default());
        o.apply(p(false, false, true, true));
        log.borrow_mut().clear();
        o.release_all();
        assert_eq!(log.borrow().len(), 2);
        assert!(log.borrow().iter().all(|e| !e.1));
        log.borrow_mut().clear();
        o.release_all();
        assert!(log.borrow().is_empty());
    }
}
