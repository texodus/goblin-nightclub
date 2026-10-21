use super::*;

const SIZE: usize = 64;

#[test]
fn runs_round_trip() {
    let mut bytes = vec![0u8; 600];
    bytes[300..310].fill(255);
    bytes[310] = 128;
    let runs = encode(&bytes);
    assert!(runs.len() < 20, "mostly one byte: {} runs", runs.len() / 2);
    assert_eq!(decode(&runs, bytes.len()), bytes);
    assert_eq!(decode(&encode(&[]), 0), Vec::<u8>::new());
}

#[test]
fn pages_are_made_only_once_seen() {
    let mut p = Pages::new(SIZE, 2, 8);
    p.begin_frame();
    assert_eq!(p.read((0, 0, 0)), None, "never seen: nothing to read");
    assert!(p.take_ops().is_empty());
    let slot = p.write((0, 0, 0)).unwrap();
    assert_eq!(p.take_ops(), vec![Op::Clear(slot)]);
    assert_eq!(p.read((0, 0, 0)), Some(slot));
    assert_eq!(p.write((0, 0, 0)), Some(slot));
    assert!(p.take_ops().is_empty(), "already there");
}

#[test]
fn the_atlas_grows_then_evicts_the_page_used_least_recently() {
    let mut p = Pages::new(SIZE, 1, 2);
    p.begin_frame();
    let a = p.write((0, 0, 0)).unwrap();
    let b = p.write((0, 1, 0)).unwrap();
    assert_eq!(p.take_ops(), vec![Op::Clear(a), Op::Grow(2), Op::Clear(b)]);
    assert_eq!(p.capacity(), 2);
    // Full, and both used this frame: no room.
    assert_eq!(p.write((0, 2, 0)), None);
    p.begin_frame();
    p.read((0, 1, 0));
    // Page (0, 0) went unused longest.
    let c = p.write((0, 2, 0)).unwrap();
    assert_eq!(c, a);
    let page = (0, 0, 0);
    assert_eq!(
        p.take_ops(),
        vec![Op::Evict { page, slot: a }, Op::Clear(a)]
    );
    // Until its pixels are back on the CPU it can't be had.
    p.begin_frame();
    assert_eq!(p.read(page), None);
    assert_eq!(p.write(page), None, "nor started afresh");
    let mut pixels = vec![0u8; SIZE];
    pixels[5] = 200;
    p.stored(page, &pixels);
    // Then it comes back, into the slot used least recently.
    p.begin_frame();
    p.read((0, 2, 0));
    let back = p.read(page).unwrap();
    assert_eq!(back, b);
    let ops = p.take_ops();
    assert_eq!(
        ops[0],
        Op::Evict {
            page: (0, 1, 0),
            slot: b
        }
    );
    assert_eq!(ops[1], Op::Upload(b, pixels));
}

#[test]
fn pages_with_nothing_seen_are_dropped() {
    let mut p = Pages::new(SIZE, 1, 1);
    p.begin_frame();
    p.write((0, 0, 0));
    p.begin_frame();
    p.write((0, 1, 0));
    p.stored((0, 0, 0), &[0; SIZE]);
    p.take_ops();
    p.begin_frame();
    assert_eq!(p.read((0, 0, 0)), None, "nothing to bring back");
    assert!(p.take_ops().is_empty());
    let slot = p.write((0, 0, 0)).unwrap();
    assert!(matches!(p.take_ops()[..], [Op::Evict { .. }, Op::Clear(s)] if s == slot));
}

#[test]
fn stale_read_backs_are_ignored() {
    let mut p = Pages::new(SIZE, 1, 1);
    p.stored((3, 0, 0), &[1; SIZE]);
    p.begin_frame();
    assert_eq!(p.read((3, 0, 0)), None, "never left, so never stored");
}
