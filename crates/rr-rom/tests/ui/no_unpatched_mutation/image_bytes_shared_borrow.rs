// US-RR-31-4: `Image::bytes` returns a shared `&[u8]`, so the bytes cannot be
// written through it (E0594).
use rr_rom::image::Image;

fn main() {
    let image = Image::new(vec![0x00, 0x11]);
    image.bytes()[0] = 0xFF;
}
