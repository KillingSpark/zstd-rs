pub fn read_little_endian_u32(raw: &[u8]) -> u32 {
    assert!(raw.len() == 4);
    let mut val = 0;
    let mut shift = 0;
    for x in raw {
        val += (*x as u32) << shift;
        shift += 8;
    }

    val
}
pub fn read_little_endian_u64(raw: &[u8]) -> u64 {
    assert!(raw.len() == 8);
    let mut val = 0;
    let mut shift = 0;
    for x in raw {
        val += (*x as u64) << shift;
        shift += 8;
    }

    val
}
