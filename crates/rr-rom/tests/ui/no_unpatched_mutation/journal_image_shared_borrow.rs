// US-RR-31-4: `Journal::image` returns a shared `&Image`, so the image cannot
// be replaced through it, nor its bytes written through it (E0594), even with
// the journal itself held mutably.
use rr_rom::image::Image;
use rr_rom::journal::Journal;

fn main() {
    let mut journal = Journal::new(Image::new(vec![0x00, 0x11]));
    *journal.image() = Image::new(vec![0xFF, 0xFF]);
    journal.image().bytes()[0] = 0xFF;
    let _ = &mut journal;
}
