// US-RR-31-4: a `Group` or `Journal` cannot be built with a struct literal
// (E0451), so no history can claim patches that were never applied to its
// image, nor an image be paired with a history that did not produce it.
use rr_rom::image::Image;
use rr_rom::journal::{Group, Journal};

fn main() {
    let _ = Group {
        patches: Box::new([]),
    };
    let _ = Journal {
        image: Image::new(vec![0x00]),
        groups: Vec::new(),
        cursor: 1,
    };
}
