// US-RR-11-2: a `RomFile` (and so an entry `Corpus::read` would call clean)
// cannot be made up with a struct literal outside rr-testkit.
use rr_testkit::corpus::RomFile;

fn forge() -> RomFile {
    RomFile {
        path: String::new(),
        size: 0,
        sha256: String::new(),
        filename_rom_id: None,
        year: None,
        year_end: None,
        region: None,
        model: None,
        trim: None,
    }
}

fn main() {}
