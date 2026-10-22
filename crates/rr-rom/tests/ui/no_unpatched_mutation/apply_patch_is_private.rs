// US-RR-31-4: `Image::apply_patch` is `pub(crate)`, so code outside rr-rom
// cannot change image bytes without the journal recording a patch: not on a
// fresh image, not on a journal's image (which is only lent as `&Image`), and
// not on the image a journal hands back from `into_image`.
use rr_rom::image::Image;
use rr_rom::journal::Journal;

fn main() {
    let mut image = Image::new(vec![0x00]);
    let _ = image.apply_patch(0, &[0x00], &[0xFF]);

    let journal = Journal::new(Image::new(vec![0x00]));
    let mut released = journal.into_image();
    let _ = released.apply_patch(0, &[0x00], &[0xFF]);
}
