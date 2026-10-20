// US-RR-11-2: a suspect entry cannot be re-made as a clean one to get
// `Image::Raw` from `Corpus::read`: `RomFile` cannot be built with a struct
// literal (its suspect field is private), not even by functional update
// from the suspect entry.
use rr_testkit::corpus::RomFile;

fn launder(file: &RomFile) -> RomFile {
    RomFile {
        path: file.path.clone(),
        ..file.clone()
    }
}

fn main() {}
