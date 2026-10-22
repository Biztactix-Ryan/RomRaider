// US-RR-31-4: a `Journal` owns its image and lends it out only as `&Image`,
// so its bytes cannot be changed, nor the image swapped, behind the
// history's back; and the recorded history cannot be edited. (Writing through
// `image()` is a borrow-check error and forging a `Group` a privacy-pass
// error, both reported only once type checking passes, so they have their own
// cases: `journal_image_shared_borrow.rs`, `forge_history.rs`.)
use rr_rom::image::Image;
use rr_rom::journal::{Journal, Origin, Patch};

fn main() {
    let mut journal = Journal::new(Image::new(vec![0x00, 0x11]));

    // The image and history are private fields.
    journal.image = Image::new(vec![0xFF, 0xFF]);
    journal.cursor = 0;
    // No mutable accessor for the image.
    let _ = journal.image_mut();
    // No `IndexMut` (nor `Index`) on the journal.
    journal[0] = 0xFF;
    // A recorded patch cannot be rewritten after the fact.
    let patch = Patch::new(0, vec![0x00], vec![0xAA], Origin::Edit).unwrap();
    journal.apply(patch).unwrap();
    journal.applied()[0].patches()[0].new = vec![0xFF].into_boxed_slice();
}
