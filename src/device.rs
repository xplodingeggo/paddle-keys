use crate::decode::{POLL, Paddle, mask_of, parse_reply};
use crate::keys::Output;
use hidapi::{HidApi, HidDevice};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const VID: u16 = 0x2DC8;
const PID: u16 = 0x310B;
const IFACE: i32 = 2;

// Only ever opens interface 2 of 2DC8:310B.
fn open() -> Result<HidDevice, String> {
    let api = HidApi::new().map_err(|e| e.to_string())?;
    let info = api
        .device_list()
        .find(|d| d.vendor_id() == VID && d.product_id() == PID && d.interface_number() == IFACE)
        .ok_or("controller 2DC8:310B interface 2 not found")?;
    info.open_device(&api).map_err(|e| e.to_string())
}

enum Poll {
    Reply([u8; 64]),
    Ignored,
    Timeout,
}

// The only write is POLL.
fn poll_once(dev: &HidDevice) -> Result<Poll, String> {
    dev.write(&POLL).map_err(|e| e.to_string())?;
    let mut buf = [0u8; 64];
    let n = dev.read_timeout(&mut buf, 50).map_err(|e| e.to_string())?;
    Ok(if n == 0 {
        Poll::Timeout
    } else if mask_of(&buf[..n]).is_some() {
        Poll::Reply(buf)
    } else {
        Poll::Ignored
    })
}

pub fn run(mut out: Output, stop: Arc<AtomicBool>, paused: Arc<AtomicBool>) {
    let mut was_paused = false;
    while !stop.load(Ordering::Relaxed) {
        let dev = match open() {
            Ok(d) => d,
            Err(_) => {
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }
        };
        let mut timeouts = 0;
        while !stop.load(Ordering::Relaxed) {
            if paused.load(Ordering::Relaxed) {
                if !was_paused {
                    out.release_all();
                    was_paused = true;
                }
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            was_paused = false;
            match poll_once(&dev) {
                Ok(Poll::Reply(b)) => {
                    timeouts = 0;
                    if let Some(p) = parse_reply(&b) {
                        out.apply(p);
                    }
                }
                Ok(Poll::Ignored) => {}
                Ok(Poll::Timeout) => {
                    timeouts += 1;
                    if timeouts >= 3 {
                        break;
                    }
                }
                Err(_) => break,
            }
            std::thread::sleep(Duration::from_millis(4));
        }
        out.release_all();
        drop(dev);
        std::thread::sleep(Duration::from_secs(1));
    }
    out.release_all();
}

pub fn probe(secs: u64) -> Result<(), String> {
    let dev = open()?;
    let end = Instant::now() + Duration::from_secs(secs);
    let (mut last, mut polls) = (None, 0u64);
    while Instant::now() < end {
        if let Poll::Reply(b) = poll_once(&dev)? {
            polls += 1;
            let m = mask_of(&b).unwrap_or(0);
            if last != Some(m) {
                last = Some(m);
                let p = parse_reply(&b).unwrap_or_default();
                let names: Vec<_> = [Paddle::L4, Paddle::R4, Paddle::PL, Paddle::PR]
                    .into_iter()
                    .filter(|&x| p.get(x))
                    .map(|x| format!("{x:?}"))
                    .collect();
                println!("mask=0x{m:08x} {}", names.join(" ").trim_end());
            }
        }
        std::thread::sleep(Duration::from_millis(4));
    }
    println!("polls: {polls}");
    Ok(())
}
