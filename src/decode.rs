#![allow(dead_code)]

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Paddle {
    L4,
    R4,
    PL,
    PR,
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Paddles {
    pub l4: bool,
    pub r4: bool,
    pub pl: bool,
    pub pr: bool,
}

impl Paddles {
    pub fn get(&self, p: Paddle) -> bool {
        match p {
            Paddle::L4 => self.l4,
            Paddle::R4 => self.r4,
            Paddle::PL => self.pl,
            Paddle::PR => self.pr,
        }
    }
}

pub const POLL: [u8; 64] = {
    let mut b = [0u8; 64];
    b[0] = 0x81;
    b[1] = 0x04;
    b[2] = 0x03;
    b[4] = 0x23;
    b[5] = 0x01;
    b
};

const REPLY_HEADER: [u8; 7] = [0x02, 0x04, 0x04, 0x00, 0x03, 0x00, 0x0c];

pub fn parse_reply(buf: &[u8]) -> Option<Paddles> {
    if !buf.starts_with(&REPLY_HEADER) {
        return None;
    }
    let mask = u32::from_le_bytes(buf.get(18..22)?.try_into().ok()?);
    Some(Paddles {
        l4: mask & 0x00200000 != 0,
        r4: mask & 0x40000000 != 0,
        pl: mask & 0x04000000 != 0,
        pr: mask & 0x02000000 != 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(mask: u32) -> Vec<u8> {
        let mut b = vec![0u8; 64];
        b[0..7].copy_from_slice(&[0x02, 0x04, 0x04, 0x00, 0x03, 0x00, 0x0c]);
        b[18..22].copy_from_slice(&mask.to_le_bytes());
        b
    }

    #[test]
    fn no_buttons() {
        assert_eq!(parse_reply(&reply(0)), Some(Paddles::default()));
    }

    #[test]
    fn each_paddle() {
        let p = parse_reply(&reply(0x00200000)).unwrap();
        assert!(p.l4 && !p.r4 && !p.pl && !p.pr);

        let p = parse_reply(&reply(0x40000000)).unwrap();
        assert!(!p.l4 && p.r4 && !p.pl && !p.pr);

        let p = parse_reply(&reply(0x04000000)).unwrap();
        assert!(!p.l4 && !p.r4 && p.pl && !p.pr);

        let p = parse_reply(&reply(0x02000000)).unwrap();
        assert!(!p.l4 && !p.r4 && !p.pl && p.pr);
    }

    #[test]
    fn face_buttons_ignored() {
        let p = parse_reply(&reply(0x00003400)).unwrap();
        assert_eq!(p, Paddles::default());
        assert!(!p.l4 && !p.r4 && !p.pl && !p.pr);
    }

    #[test]
    fn combo() {
        let p = parse_reply(&reply(0x04000000 | 0x40000000)).unwrap();
        assert!(p.pl && p.r4);
        assert!(!p.l4 && !p.pr);
    }

    #[test]
    fn wrong_header_is_none() {
        let mut b = reply(0);
        b[0] = 0x01;
        assert_eq!(parse_reply(&b), None);
    }

    #[test]
    fn short_buffer_is_none() {
        let b = reply(0);
        assert_eq!(parse_reply(&b[..21]), None);
    }

    #[test]
    fn poll_buffer_shape() {
        assert_eq!(POLL[0..6], [0x81, 4, 3, 0, 0x23, 1]);
        assert_eq!(POLL.len(), 64);
        assert!(POLL[6..].iter().all(|&b| b == 0));
    }

    #[test]
    fn real_capture() {
        let mut b = reply(0);
        b[22..26].copy_from_slice(&[0xff, 0x4e, 0x80, 0x7f]);
        assert_eq!(parse_reply(&b), Some(Paddles::default()));
    }
}
