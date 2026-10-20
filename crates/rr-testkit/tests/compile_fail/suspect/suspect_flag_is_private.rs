// US-RR-11-2: a `RomFile`'s suspect table is not a public field, so a
// consumer cannot check it (or forget to) outside `Entry` / `Image`.
use rr_testkit::corpus::RomFile;

fn is_suspect(file: &RomFile) -> bool {
    file.suspect.is_some()
}

fn main() {}
