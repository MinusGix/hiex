use hiex::Hiex;
use std::fs::OpenOptions;

fn main() {
    // Open file that we want to edit.
    let mut original_file = OpenOptions::new()
        .read(true)
        .write(true)
        .open("./examples/test.txt")
        .expect("Failed to open file.");

    // Make a temporary file.
    // We make a temporary file, because otherwise the hex editor would directly write
    // to `original_file`. This may be what you want at times, but it is also common to
    // want to explicitly have to save. The recommended way is to create a temp file.
    let mut editing_file = tempfile::NamedTempFile::new().expect("Failed to create temporary file");
    // Copy over the data to the resulting file.
    std::io::copy(&mut original_file, &mut editing_file)
        .expect("Failed to copy file to temporary.");

    let mut hex =
        Hiex::<_, ()>::from_reader(editing_file).expect("Failed to create hex editor instance.");
    let data = hex.read_amount_at(0, 420).expect("Failed to read");
    println!("Data size: {}", data.len());
    for c in data {
        if c >= 32 && c <= 127 {
            print!("{}", c as char);
        } else {
            print!("\\u{{{:x}}}", c);
        }
    }
}
