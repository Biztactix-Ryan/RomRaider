// US-RR-31-4 positive control: the same changes as the compile-fail cases,
// made the supported way, through a patch the journal records, compile and
// run. So each compile-fail case fails for the reason its `.stderr` records,
// not because the harness cannot build anything.
use rr_rom::image::Image;
use rr_rom::journal::{Journal, Origin, Patch};

fn main() {
    let mut journal = Journal::new(Image::new(vec![0x00, 0x11]));
    let patch = Patch::replacing(journal.image(), 0, vec![0xFF], Origin::Edit).unwrap();
    journal.apply(patch).unwrap();
    assert_eq!(journal.image().bytes(), &[0xFF, 0x11]);
    assert_eq!(journal.applied().len(), 1);
    assert_eq!(journal.applied()[0].patches()[0].new_bytes(), &[0xFF]);

    // The change is recorded, so it can be undone.
    journal.undo().unwrap();
    assert_eq!(journal.image().bytes(), &[0x00, 0x11]);
    assert!(!journal.image().is_modified());

    // Read-only access to the bytes is public.
    let image = journal.into_image();
    assert_eq!(image.bytes(), image.original());
    let copy: Vec<u8> = image.bytes().to_vec();
    assert_eq!(copy, vec![0x00, 0x11]);
}
