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
/// Models whose status byte reliably says both earbuds are in the case.
pub fn has_case_layout(id: u16) -> bool {
    matches!(
        id,
        0x2002 | 0x200F | 0x2013 | 0x200E | 0x2014 | 0x2024 | 0x2019 | 0x201B | 0x200B
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
        let dual = has_case_layout(id);
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
            // Only models with a verified status layout.
            cased: dual && both_case,
            approximate: true,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn earbuds_in_the_case_are_reported_until_one_leaves() {
        for (status, cased) in [(0x24, true), (0x04, true), (0x22, false), (0x62, false)] {
            let mut frame = [0u8; 27];
            frame[..9].copy_from_slice(&[7, 25, 1, 0x14, 0x20, status, 0x88, 0x08, 0]);
            assert_eq!(
                parse(&frame, 1, -50, 1000).unwrap().cased,
                cased,
                "{status:#x}"
            );
        }
        for ungated in [
            0x200au16, 0x201f, 0x2003, 0x2005, 0x2006, 0x2009, 0x2010, 0x2027,
        ] {
            let mut frame = [0u8; 27];
            frame[..9].copy_from_slice(&[
                7,
                25,
                1,
                ungated as u8,
                (ungated >> 8) as u8,
                0x04,
                0x88,
                0x08,
                0,
            ]);
            assert!(!parse(&frame, 1, -50, 1000).unwrap().cased, "{ungated:#x}");
        }
    }
    #[test]
    fn physical_battery_and_charging_sides_survive_primary_switches() {
        for id in [
            0x2002u16, 0x200f, 0x2013, 0x200e, 0x2014, 0x2024, 0x2019, 0x201b,
        ] {
            for physical_flags in 0..8u8 {
                let mut batteries = crate::model::Batteries::default();
                for (status, levels, wire_flags) in [
                    (0x24, 0x38, physical_flags),
                    (
                        0x04,
                        0x83,
                        [0, 2, 1, 3, 4, 6, 5, 7][physical_flags as usize],
                    ),
                ] {
                    let mut frame = [0u8; 27];
                    frame[..9].copy_from_slice(&[
                        7,
                        25,
                        1,
                        id as u8,
                        (id >> 8) as u8,
                        status,
                        levels,
                        (wire_flags << 4) | 6,
                        0,
                    ]);
                    let reading = parse(&frame, 1, -50, 1000).unwrap();
                    assert_eq!(reading.battery, [Some(80), Some(30), Some(60)]);
                    assert_eq!(
                        reading.charging,
                        [
                            Some(physical_flags & 1 != 0),
                            Some(physical_flags & 2 != 0),
                            Some(physical_flags & 4 != 0),
                        ]
                    );
                    batteries.remember(&reading);
                    assert_eq!(batteries.value(0), "≈80%");
                    assert_eq!(batteries.value(1), "≈30%");
                    assert_eq!(batteries.value(2), "≈60%");
                    assert_eq!(batteries.tray_level(), Some(30));
                }
            }
        }
    }
    #[test]
    fn case_and_primary_bits_do_not_reverse_the_ear_sensors() {
        for id in [0x2002u16, 0x200f, 0x2013, 0x200e] {
            for (base, left_bit, right_bit) in
                [(0x00, 8, 2), (0x20, 2, 8), (0x40, 2, 8), (0x60, 8, 2)]
            {
                for ear_bits in [0, 2, 8, 10] {
                    for both_case in [0, 4] {
                        let mut frame = [0u8; 27];
                        frame[..9].copy_from_slice(&[
                            7,
                            25,
                            1,
                            id as u8,
                            (id >> 8) as u8,
                            base | ear_bits | both_case,
                            0x38,
                            6,
                            0,
                        ]);
                        let reading = parse(&frame, 1, -50, 1000).unwrap();
                        assert_eq!(
                            reading.ears,
                            [
                                Some(both_case == 0 && ear_bits & left_bit != 0),
                                Some(both_case == 0 && ear_bits & right_bit != 0),
                            ],
                            "model {id:x}, status {:x}",
                            frame[5]
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn h2_secondary_sensors_remain_unknown_until_a_reliable_case_frame() {
        for id in [0x2014u16, 0x2024, 0x2019, 0x201b] {
            for (status, expected) in [
                (0x02, [None, Some(true)]),
                (0x22, [Some(true), None]),
                (0x00, [None, Some(false)]),
                (0x20, [Some(false), None]),
                (0x04, [Some(false), Some(false)]),
                (0x24, [Some(false), Some(false)]),
            ] {
                let mut frame = [0u8; 27];
                frame[..9].copy_from_slice(&[
                    7,
                    25,
                    1,
                    id as u8,
                    (id >> 8) as u8,
                    status,
                    0x38,
                    6,
                    0,
                ]);
                assert_eq!(parse(&frame, 1, -50, 1000).unwrap().ears, expected);
            }
        }
    }
    #[test]
    fn unknown_battery_sides_never_invent_charge_or_ear_states() {
        for unknown in 11..16u8 {
            for primary_left in [false, true] {
                let mut frame = [0u8; 27];
                frame[..9].copy_from_slice(&[
                    7,
                    25,
                    1,
                    0x1b,
                    0x20,
                    if primary_left { 0x22 } else { 0x02 },
                    (unknown << 4) | 7,
                    0x7f,
                    8,
                ]);
                let reading = parse(&frame, 1, -50, 1000).unwrap();
                let known = usize::from(!primary_left);
                let missing = 1 - known;
                assert_eq!(reading.battery[known], Some(70));
                assert_eq!(reading.charging[known], Some(true));
                assert_eq!(reading.ears[known], Some(true));
                assert_eq!(reading.battery[missing], None);
                assert_eq!(reading.charging[missing], None);
                assert_eq!(reading.ears[missing], None);
                assert_eq!(reading.battery[2], None);
                assert_eq!(reading.charging[2], None);
            }
        }
    }
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
