//! The markers are inert: marked free functions, inherent methods and trait
//! methods compile and behave exactly as unmarked ones.

use rr_write_marker::{rr_not_write, rr_write};

/// Stand-in for `rr_core::capability::RomWrite` (the marker crate does not
/// depend on rr-core; only the source shape matters to check-writes).
struct RomWrite;

#[rr_write]
fn save(buf: &mut Vec<u8>, byte: u8, _token: RomWrite) {
    buf.push(byte);
}

#[rr_not_write("reads only: copies the buffer")]
fn read_back(buf: &[u8]) -> Vec<u8> {
    buf.to_vec()
}

struct Image(Vec<u8>);

impl Image {
    #[rr_write]
    fn write_byte(&mut self, byte: u8, _token: &RomWrite) {
        self.0.push(byte);
    }
}

trait Sink {
    #[rr_write]
    fn store(&mut self, byte: u8, token: RomWrite);
}

impl Sink for Image {
    #[rr_write]
    fn store(&mut self, byte: u8, _token: RomWrite) {
        self.0.push(byte);
    }
}

#[test]
fn marked_items_are_unchanged() {
    let mut buf = Vec::new();
    save(&mut buf, 1, RomWrite);
    assert_eq!(read_back(&buf), [1]);

    let mut img = Image(buf);
    img.write_byte(2, &RomWrite);
    img.store(3, RomWrite);
    assert_eq!(img.0, [1, 2, 3]);
}
