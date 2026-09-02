use crate::adapters::PreparedMaterialArena;

#[derive(Clone, Eq, Hash, PartialEq)]
struct DummyReservation(u8);

struct StagedReservation {
    key: DummyReservation,
    cleanup: Option<Box<dyn FnOnce() + Send>>,
}

impl Drop for StagedReservation {
    fn drop(&mut self) {
        if let Some(cleanup) = self.cleanup.take() {
            cleanup();
        }
    }
}

fn staged(arena: &PreparedMaterialArena<DummyReservation>) -> StagedReservation {
    let key = DummyReservation(0x38);
    let cleanup = arena.stage_for_test(&key, vec![0x38, 0x39, 0x40]);
    StagedReservation {
        key,
        cleanup: Some(cleanup),
    }
}

#[test]
fn dropping_an_uncommitted_reservation_removes_and_zeroizes_its_arena_entry() {
    let arena = PreparedMaterialArena::default();
    let reservation = staged(&arena);
    assert_eq!(arena.retained_batch_count_for_test(), 1);

    drop(reservation);

    assert_eq!(arena.retained_batch_count_for_test(), 0);
    assert!(arena.all_retained_bytes_are_zero_for_test());
}

#[test]
fn commit_consumes_a_prepared_reservation_exactly_once() {
    let arena = PreparedMaterialArena::default();
    let reservation = staged(&arena);

    assert!(arena.consume_for_test(&reservation.key).is_ok());
    assert!(arena.consume_for_test(&reservation.key).is_err());
    assert_eq!(arena.retained_batch_count_for_test(), 0);
}

#[test]
fn a_reservation_from_another_arena_is_rejected_without_consuming_the_owner_entry() {
    let owner = PreparedMaterialArena::default();
    let foreign = PreparedMaterialArena::default();
    let reservation = staged(&owner);

    assert!(foreign.consume_for_test(&reservation.key).is_err());
    assert_eq!(owner.retained_batch_count_for_test(), 1);
}

#[test]
fn an_unknown_reservation_is_rejected_without_mutating_the_arena() {
    let arena = PreparedMaterialArena::<DummyReservation>::default();
    let unknown = DummyReservation(0x39);

    assert!(arena.consume_for_test(&unknown).is_err());
    assert_eq!(arena.retained_batch_count_for_test(), 0);
}

#[test]
fn rollback_discards_and_zeroizes_the_whole_prepared_reservation() {
    let arena = PreparedMaterialArena::default();
    let reservation = staged(&arena);

    arena
        .discard_for_test(&reservation.key)
        .expect("owned reservation discards");

    assert_eq!(arena.retained_batch_count_for_test(), 0);
    assert!(arena.all_retained_bytes_are_zero_for_test());
}
