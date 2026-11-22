//! Message translation between the 8BitDo's Xbox 360 (XNA) protocol and the GIP protocol
//! SDL's GIP driver inside Steam expects from an Xbox Elite 2. Pure functions, unit tested.

/// Sideband buffers start with the 8-byte device id, the endpoint, the total size and 3 more bytes.
pub const PREFIX: usize = 13;
/// Elite 2 (firmware 5.17+) input payload size; SDL reads paddles at 14 and the profile at 20.
pub const INPUT_LEN: usize = 47;
pub const INPUT_MSG: usize = 4 + INPUT_LEN;
const PADDLE_OFFSET: usize = 4 + 14;

/// Hello for 045E:0B00 with firmware 5.17, which makes SDL pick its XBE2_5 paddle format.
pub fn hello(seq: u8) -> [u8; 32] {
    [
        0x02, 0x20, seq, 0x1c, // system message, 28 bytes
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x7e, // device id (any)
        0x5e, 0x04, 0x00, 0x0b, // 045E:0B00
        5, 0, 17, 0, 0, 0, 0, 0, // firmware 5.17.0 rev 0
        1, 0, 1, 0, 1, 0, 1, 0, // hardware, RF, security and GIP versions
    ]
}

/// Guide button state. No ACME flag, so SDL never writes an ack to the pad.
pub fn guide(seq: u8, down: bool) -> [u8; 6] {
    [0x07, 0x20, seq, 0x02, down as u8, 0x5b]
}

/// One 360 input report: `00 14 b2 b3 lt rt lx ly rx ry` with 16-bit sticks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pad360 {
    pub b2: u8,
    pub b3: u8,
    pub lt: u8,
    pub rt: u8,
    pub sticks: [u8; 8],
}

impl Pad360 {
    pub fn parse(msg: &[u8]) -> Option<Pad360> {
        if msg.len() < 14 || msg[0] != 0x00 || msg[1] != 0x14 {
            return None;
        }
        let mut sticks = [0u8; 8];
        sticks.copy_from_slice(&msg[6..14]);
        Some(Pad360 { b2: msg[2], b3: msg[3], lt: msg[4], rt: msg[5], sticks })
    }

    pub fn guide(&self) -> bool {
        self.b3 & 0x04 != 0
    }
}

/// GIP input report. `paddles` bits 0-3 are P1-P4.
pub fn input(seq: u8, p: &Pad360, paddles: u8) -> [u8; INPUT_MSG] {
    let mut m = [0u8; INPUT_MSG];
    m[..4].copy_from_slice(&[0x20, 0x00, seq, INPUT_LEN as u8]);
    let bit = |src: u8, mask: u8, dst: u8| if src & mask != 0 { dst } else { 0 };
    // byte 0: menu, view, A, B, X, Y
    m[4] = bit(p.b2, 0x10, 0x04) | bit(p.b2, 0x20, 0x08)
        | bit(p.b3, 0x10, 0x10) | bit(p.b3, 0x20, 0x20) | bit(p.b3, 0x40, 0x40) | bit(p.b3, 0x80, 0x80);
    // byte 1: dpad up/down/left/right, LB, RB, LS, RS
    m[5] = (p.b2 & 0x0f) | bit(p.b3, 0x01, 0x10) | bit(p.b3, 0x02, 0x20) | (p.b2 & 0xc0);
    let trig = |v: u8| (((v as u32 * 1023 + 127) / 255) as u16).to_le_bytes();
    m[6..8].copy_from_slice(&trig(p.lt));
    m[8..10].copy_from_slice(&trig(p.rt));
    m[10..18].copy_from_slice(&p.sticks);
    m[PADDLE_OFFSET] = paddles & 0x0f;
    m
}

/// Re-stamps a previous input report with a new sequence number and paddle state.
pub fn restamp(mut m: [u8; INPUT_MSG], seq: u8, paddles: u8) -> [u8; INPUT_MSG] {
    m[2] = seq;
    m[PADDLE_OFFSET] = paddles & 0x0f;
    m
}

/// Left/right motor levels (0-255) from a GIP direct-motor message (levels 0-100), if it is one.
pub fn rumble_levels(gip: &[u8]) -> Option<(u8, u8)> {
    if gip.len() < 4 + 6 || gip[0] != 0x09 {
        return None;
    }
    let scale = |v: u8| (v.min(100) as u16 * 255 / 100) as u8;
    Some((scale(gip[4 + 4]), scale(gip[4 + 5])))
}

/// 360 rumble output report.
pub fn rumble_360(left: u8, right: u8) -> [u8; 8] {
    [0x00, 0x08, 0x00, left, right, 0x00, 0x00, 0x00]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad(b2: u8, b3: u8, lt: u8, rt: u8) -> Pad360 {
        Pad360 { b2, b3, lt, rt, sticks: [1, 2, 3, 4, 5, 6, 7, 8] }
    }

    #[test]
    fn hello_is_28_byte_system_message_for_elite2_fw_5_17() {
        let h = hello(9);
        assert_eq!(h[..4], [0x02, 0x20, 9, 28]);
        assert_eq!(h.len() - 4, 28);
        assert_eq!(h[12..16], [0x5e, 0x04, 0x00, 0x0b]);
        assert_eq!((h[16], h[18]), (5, 17));
    }

    #[test]
    fn guide_message() {
        assert_eq!(guide(3, true), [0x07, 0x20, 3, 0x02, 1, 0x5b]);
        assert_eq!(guide(4, false)[4], 0);
    }

    #[test]
    fn parses_360_report_only() {
        let r = [0x00, 0x14, 0x10, 0x84, 0x20, 0x30, 1, 2, 3, 4, 5, 6, 7, 8, 0, 0, 0, 0, 0, 0];
        let p = Pad360::parse(&r).unwrap();
        assert_eq!(p, Pad360 { b2: 0x10, b3: 0x84, lt: 0x20, rt: 0x30, sticks: [1, 2, 3, 4, 5, 6, 7, 8] });
        assert!(p.guide());
        assert_eq!(Pad360::parse(&[0x01, 0x03, 0x06]), None);
        assert_eq!(Pad360::parse(&r[..13]), None);
    }

    #[test]
    fn face_and_menu_buttons() {
        // A B X Y
        assert_eq!(input(0, &pad(0, 0xf0, 0, 0), 0)[4], 0xf0);
        // start -> menu, back -> view
        assert_eq!(input(0, &pad(0x10, 0, 0, 0), 0)[4], 0x04);
        assert_eq!(input(0, &pad(0x20, 0, 0, 0), 0)[4], 0x08);
    }

    #[test]
    fn dpad_shoulders_and_stick_clicks() {
        assert_eq!(input(0, &pad(0x0f, 0, 0, 0), 0)[5], 0x0f);
        assert_eq!(input(0, &pad(0, 0x01, 0, 0), 0)[5], 0x10);
        assert_eq!(input(0, &pad(0, 0x02, 0, 0), 0)[5], 0x20);
        assert_eq!(input(0, &pad(0xc0, 0, 0, 0), 0)[5], 0xc0);
        // guide never leaks into the input report
        assert_eq!(input(0, &pad(0, 0x04, 0, 0), 0)[4..6], [0, 0]);
    }

    #[test]
    fn triggers_scale_to_10_bit() {
        let m = input(0, &pad(0, 0, 255, 0), 0);
        assert_eq!(u16::from_le_bytes([m[6], m[7]]), 1023);
        assert_eq!(u16::from_le_bytes([m[8], m[9]]), 0);
    }

    #[test]
    fn header_sticks_paddles_and_profile() {
        let m = input(7, &pad(0, 0, 0, 0), 0x1f);
        assert_eq!(m[..4], [0x20, 0x00, 7, 47]);
        assert_eq!(m[10..18], [1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(m[4 + 14], 0x0f);
        assert_eq!(m[4 + 20], 0, "non-zero profile byte makes SDL clear the paddles");
        assert_eq!(m.len(), 51);
    }

    #[test]
    fn restamp_changes_only_seq_and_paddles() {
        let m = input(1, &pad(0x10, 0x10, 9, 9), 0);
        let r = restamp(m, 2, 0x08);
        assert_eq!((r[2], r[18]), (2, 0x08));
        assert_eq!(r[4..18], m[4..18]);
    }

    #[test]
    fn rumble_from_sdl_capture() {
        // 0x2aac08 write captured from Steam: 09 00 00 09 | 00 0f 00 00 32 32 ff 00 eb
        let gip = [0x09, 0x00, 0x00, 0x09, 0x00, 0x0f, 0x00, 0x00, 0x32, 0x32, 0xff, 0x00, 0xeb];
        assert_eq!(rumble_levels(&gip), Some((127, 127)));
        assert_eq!(rumble_levels(&[0x0e, 0x00, 0x00, 0x05, 0, 0, 0, 0xff, 0xff]), None);
        assert_eq!(rumble_360(255, 1), [0, 8, 0, 255, 1, 0, 0, 0]);
    }
}
