// US-RR-31-4: an `Image` gives out its bytes only as `&[u8]`. There is no
// public field, no `bytes_mut`, and no `DerefMut`, `IndexMut`, `AsMut`,
// `BorrowMut`, `&mut` iteration or `Extend` to write through.
use std::borrow::BorrowMut;

use rr_rom::image::Image;

fn main() {
    let mut image = Image::new(vec![0x00, 0x11]);

    // The buffer is a private field.
    image.bytes[0] = 0xFF;
    // No mutable accessor.
    let _ = image.bytes_mut();
    // No `IndexMut` (nor `Index`).
    image[0] = 0xFF;
    // No `DerefMut` (nor `Deref`) to `[u8]` / `Vec<u8>`.
    (*image)[0] = 0xFF;
    // No `AsMut<[u8]>`.
    let _: &mut [u8] = image.as_mut();
    // No `BorrowMut<[u8]>`.
    let _: &mut [u8] = <Image as BorrowMut<[u8]>>::borrow_mut(&mut image);
    // No `IntoIterator` for `&mut Image`.
    for byte in &mut image {
        *byte = 0xFF;
    }
    // No `Extend<u8>`.
    image.extend([0xFF]);
}
