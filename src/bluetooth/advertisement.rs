use crate::model::Reading;

pub fn model_name(id: u16) -> Option<&'static str> {
    Some(match id {
        0x2002 => "AirPods",
        0x200F => "AirPods 2",
        0x2013 => "AirPods 3",
        0x200E => "AirPods Pro",
        0x2014 | 0x2024 => "AirPods Pro 2",
        0x2027 => "AirPods Pro 3",
        0x200A | 0x201F => "AirPods Max",
        0x2019 => "AirPods 4",
        0x201B => "AirPods 4 (ANC)",
        0x2003 => "Powerbeats 3",
        0x2005 => "Beats X",
        0x2006 => "Beats Solo 3",
        0x2009 => "Beats Studio 3",
        0x200B => "Powerbeats Pro",
        0x2010 => "Beats Flex",
        _ => return None,
    })
}
pub fn is_airpods(id: u16) -> bool {
    matches!(
        id,
        0x2002
            | 0x200F
            | 0x2013
            | 0x200E
            | 0x2014
            | 0x2024
            | 0x2027
            | 0x200A
            | 0x201F
            | 0x2019
            | 0x201B
    )
}
pub fn is_max(id: u16) -> bool {
    matches!(id, 0x200A | 0x201F)
}
fn level(n: u8) -> Option<u8> {
    (n <= 10).then_some(n.saturating_mul(10))
}
pub fn parse(bytes: &[u8], address: u64, signal: i16, at: u64) -> Option<Reading> {
    let mut offset = 0;
    while offset + 2 <= bytes.len() {
        let n = usize::from(bytes[offset + 1]) + 2;
        let data = bytes.get(offset..offset + n)?;
        offset += n;
        if data[0] != 7 || n < 11 {
            continue;
        }
        let id = u16::from_le_bytes([data[3], data[4]]);
        let name = model_name(id)?;
        let status = data[5];
        let primary_left = status & 0x20 != 0;
        let primary = level(data[6] & 15);
        let secondary = level(data[6] >> 4);
        let mut battery = if primary_left {
            [primary, secondary, level(data[7] & 15)]
        } else {
            [secondary, primary, level(data[7] & 15)]
        };
        let flags = data[7] >> 4;
        let mut charging = [
            battery[0].map(|_| flags & if primary_left { 1 } else { 2 } != 0),
            battery[1].map(|_| flags & if primary_left { 2 } else { 1 } != 0),
            battery[2].map(|_| flags & 4 != 0),
        ];
        let both_case = status & 4 != 0;
        let broadcaster_case = status & 0x40 != 0;
        let flip = !primary_left ^ broadcaster_case;
        let dual = matches!(
            id,
            0x2002 | 0x200F | 0x2013 | 0x200E | 0x2014 | 0x2024 | 0x2019 | 0x201B | 0x200B
        );
        let mut ears = [None, None];
        if data[1] == 25 && dual {
            ears = [
                battery[0].map(|_| !both_case && status & if flip { 8 } else { 2 } != 0),
                battery[1].map(|_| !both_case && status & if flip { 2 } else { 8 } != 0),
            ];
            // H2 frames may describe just the broadcaster; never infer the other ear's sensor.
            if !both_case && matches!(id, 0x2014 | 0x2024 | 0x2019 | 0x201B) {
                ears[usize::from(primary_left)] = None;
            }
        }
        if is_max(id) {
            battery = [primary.or(secondary), None, None];
            charging = [battery[0].map(|_| flags & 1 != 0), None, None];
        }
        return Some(Reading {
            address,
            model: id,
            name: name.into(),
            signal,
            at,
            battery,
            charging,
            ears,
            lid: (!is_max(id) && (both_case || broadcaster_case)).then_some(data[8] & 8 == 0),
            approximate: true,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn truncation_never_reads_outside_the_frame() {
        let frame = [
            7, 25, 1, 0x1b, 0x20, 0x24, 0x98, 0x79, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0,
        ];
        for n in 0..frame.len() {
            assert!(parse(&frame[..n], 1, -50, 0).is_none());
        }
        assert!(parse(&frame, 1, -50, 0).is_some());
    }
    #[test]
    fn battery_sides_and_charge_flags_are_consistent() {
        let mut f = [0u8; 27];
        f[..9].copy_from_slice(&[7, 25, 1, 0x1b, 0x20, 0x24, 0x98, 0x79, 0]);
        let r = parse(&f, 1, -50, 0).unwrap();
        assert_eq!(r.battery, [Some(80), Some(90), Some(90)]);
        assert_eq!(r.charging, [Some(true); 3]);
        assert_eq!(r.ears, [Some(false); 2]);
        assert_eq!(r.lid, Some(true));
        f[5] = 4;
        let r = parse(&f, 1, -50, 0).unwrap();
        assert_eq!(r.battery[..2], [Some(90), Some(80)]);
    }
    #[test]
    fn unknown_and_sentinel_levels_are_not_invented() {
        let mut f = [0u8; 27];
        f[..9].copy_from_slice(&[7, 25, 1, 0x27, 0x20, 0x24, 0xff, 0xff, 0]);
        let r = parse(&f, 1, -50, 0).unwrap();
        assert_eq!(r.battery, [None; 3]);
        assert_eq!(r.ears, [None; 2]);
        f[3] = 0xff;
        assert!(parse(&f, 1, -50, 0).is_none());
    }
    #[test]
    fn captured_airpods_4_left_primary_frame_reports_the_left_ear() {
        let mut frame = [0u8; 27];
        // Public header from the live capture; its private tail is irrelevant to this decoder.
        frame[..11].copy_from_slice(&[
            0x07, 0x19, 0x01, 0x1b, 0x20, 0x22, 0xf5, 0x8f, 0x10, 0x00, 0x05,
        ]);
        let worn = parse(&frame, 1, -50, 1000).unwrap();
        assert_eq!(worn.battery[..2], [Some(50), None]);
        assert_eq!(worn.ears, [Some(true), None]);
        frame[5] &= !2;
        assert_eq!(
            parse(&frame, 1, -50, 1100).unwrap().ears,
            [Some(false), None]
        );
    }
    #[test]
    fn primary_ear_uses_bit_one_on_both_physical_sides() {
        for id in [0x2002u16, 0x2013, 0x200e, 0x2014, 0x2024, 0x2019, 0x201b] {
            for primary_left in [false, true] {
                let mut frame = [0u8; 27];
                frame[..9].copy_from_slice(&[
                    7,
                    25,
                    1,
                    id as u8,
                    (id >> 8) as u8,
                    0,
                    0xf5,
                    0x0f,
                    8,
                ]);
                frame[5] = 2 | if primary_left { 0x20 } else { 0 };
                let r = parse(&frame, 1, -50, 1000).unwrap();
                assert_eq!(
                    r.ears[usize::from(!primary_left)],
                    Some(true),
                    "model {id:x}"
                );
                assert_eq!(r.ears[usize::from(primary_left)], None);
            }
        }
    }
}
