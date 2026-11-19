//! 1-bit PBM (P4) images for the LCD board (#85): a one-line ASCII header,
//! then rows of 1 bit per pixel, most significant bit first, 1 = black.

/// Gray values below this become black.
const THRESHOLD: u8 = 128;

/// `P4 <w> <h>` and a newline.
pub fn header(w: u32, h: u32) -> String {
    format!("P4 {w} {h}\n")
}

/// The whole P4 file for a `w`×`h` gray image: one byte per pixel, top row
/// first, 0 black and 255 white. Each row is padded to whole bytes with
/// white bits.
pub fn encode(gray: &[u8], w: u32, h: u32) -> Vec<u8> {
    let mut out = header(w, h).into_bytes();
    for row in gray.chunks(w as usize).take(h as usize) {
        out.extend(pack_row(row));
    }
    out
}

fn pack_row(row: &[u8]) -> Vec<u8> {
    row.chunks(8)
        .map(|px| {
            px.iter()
                .enumerate()
                .filter(|(_, &g)| g < THRESHOLD)
                .fold(0u8, |byte, (i, _)| byte | (0x80 >> i))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{encode, header};

    const B: u8 = 0;
    const W: u8 = 255;

    #[test]
    fn header_is_one_ascii_line() {
        assert_eq!(header(400, 120), "P4 400 120\n");
    }

    #[test]
    fn black_is_bit_one_msb_first() {
        let gray = [B, W, W, W, W, W, W, B];
        assert_eq!(encode(&gray, 8, 1), b"P4 8 1\n\x81");
    }

    #[test]
    fn rows_are_padded_to_whole_bytes_with_white() {
        // 10 wide: two bytes per row, the last 6 bits of each row are 0.
        let mut gray = vec![B; 10];
        gray.extend([W; 9]);
        gray.push(B);
        let pbm = encode(&gray, 10, 2);
        let body = &pbm[header(10, 2).len()..];
        assert_eq!(body, [0xFF, 0xC0, 0x00, 0x40]);
    }

    #[test]
    fn threshold_splits_gray_at_128() {
        let gray = [127, 128, 0, 255, 200, 50, 129, 1];
        assert_eq!(encode(&gray, 8, 1).last(), Some(&0b1010_0101));
    }

    #[test]
    fn size_is_header_plus_rows() {
        let gray = vec![W; 400 * 120];
        assert_eq!(
            encode(&gray, 400, 120).len(),
            "P4 400 120\n".len() + 50 * 120
        );
    }
}
