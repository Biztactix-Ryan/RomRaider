// US-RR-11-2: `Corpus::read` does not hand back bytes that can be used as a
// raw image without matching `Image::Raw` / `Image::Suspect` first: it is not
// a `Vec<u8>`, does not deref, index or iterate as one, and has no `AsRef`.
use rr_testkit::corpus::{Corpus, RomFile};

fn as_vec(corpus: &Corpus, file: &RomFile) -> Vec<u8> {
    corpus.read(file).unwrap()
}

fn as_slice(corpus: &Corpus, file: &RomFile) -> usize {
    let image = corpus.read(file).unwrap();
    image.len() + image[0] as usize
}

fn as_ref(corpus: &Corpus, file: &RomFile) -> usize {
    let image = corpus.read(file).unwrap();
    AsRef::<[u8]>::as_ref(&image).len()
}

fn main() {}
