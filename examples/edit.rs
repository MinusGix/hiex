use hiex::{action::ActionError, EditAction, Hiex};
use std::io::{Cursor, Seek, SeekFrom};

fn print_bytes(data: &[u8]) {
    for c in data.iter().copied() {
        if c >= 32 && c <= 127 {
            print!("{}", c as char);
        } else {
            print!("\\u{{{:x}}}", c);
        }
    }
    println!();
}

fn main() {
    let mut data: Vec<u8> = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ".to_vec();
    let mut destination_cursor: Cursor<&mut Vec<u8>> = Cursor::new(&mut data);

    // Make a copy. This will have our edits until we save.
    let mut copy_cursor = std::io::Cursor::new(Vec::new());
    std::io::copy(&mut destination_cursor, &mut copy_cursor)
        .expect("Failed to copy data to temp data");
    destination_cursor
        .seek(SeekFrom::Start(0))
        .expect("Failed to reset seek position");

    let mut hex = Hiex::from_reader(copy_cursor).expect("Failed to create hex editor instance.");
    let data = hex.read_amount_at(0, 420).expect("Failed to read");
    assert_eq!(data.len(), 26);
    print!("Data: ");
    print_bytes(&data);

    hex.add_action(EditAction::new(1, b"ZDX".to_vec()), ())
        .expect("Failed to write");
    // hex.write_at(1, b"ZDX").expect("Failed to write data");
    let data = hex.read_amount_at(0, 10).expect("Failed to read");
    assert_eq!(data.len(), 10);
    assert_eq!(data, b"AZDXEFGHIJ");
    print!("Data: ");
    print_bytes(&data);

    let length = hex.length().expect("Failed to get length");
    let _result = hex
        .add_action(EditAction::new(length, b"0123".to_vec()), ())
        .expect_err("Expected error when trying to edit past end of file.");

    hex.save_to(&mut destination_cursor)
        .expect("Failed to save to writer");

    let data = destination_cursor.into_inner();
    assert_eq!(data, b"AZDXEFGHIJKLMNOPQRSTUVWXYZ");
    print!("Dest Data: ");
    print_bytes(data);

    hex.add_action(EditAction::new(5, b"01".to_vec()), ())
        .expect("Failed to add action");
    let data = hex.read_amount_at(0, 26).expect("Failed to read");
    assert_eq!(data.len(), 26);
    assert_eq!(data, b"AZDXE01HIJKLMNOPQRSTUVWXYZ");
    print!("Data: ");
    print_bytes(&data);

    hex.undo(()).expect("Failed to undo");
    let data = hex.read_amount_at(0, 10).expect("Failed to read");
    assert_eq!(data.len(), 10);
    assert_eq!(data, b"AZDXEFGHIJ");
    print!("Data: ");
    print_bytes(&data);

    hex.undo(()).expect("Failed to undo");
    let data = hex.read_amount_at(0, 10).expect("Failed to read");
    assert_eq!(data.len(), 10);
    assert_eq!(data, b"ABCDEFGHIJ");
    print!("Data: ");
    print_bytes(&data);
}
