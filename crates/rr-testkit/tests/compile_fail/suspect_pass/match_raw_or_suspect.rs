// US-RR-11-2 positive control: matching the image gets at the bytes, raw or
// (explicitly) suspect, and the embedded image of a suspect.
use rr_testkit::corpus::{Corpus, CorpusError, Entry, Image};

fn bytes(corpus: &Corpus) -> Result<usize, CorpusError> {
    let mut total = 0;
    for entry in corpus.manifest().entries() {
        if let Entry::Suspect { suspect, .. } = entry {
            assert!(!suspect.reason.is_empty());
        }
        total += match corpus.read(entry.file())? {
            Image::Raw(bytes) => bytes.len(),
            Image::Suspect { bytes, suspect } => suspect.embedded(&bytes).map_or(0, <[u8]>::len),
        };
    }
    Ok(total)
}

fn main() {
    let _ = bytes;
}
