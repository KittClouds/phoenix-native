use phoenix_audio::*;
use phoenix_tts_contract::{BLOCK_FRAMES, POOL_BLOCKS};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};
thread_local! { static MEASURING: Cell<bool> = const { Cell::new(false) }; static ALLOCS: Cell<usize> = const { Cell::new(0) }; }
struct AllocationProbe;
unsafe impl GlobalAlloc for AllocationProbe {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        MEASURING.with(|m| {
            if m.get() {
                ALLOCS.with(|n| n.set(n.get() + 1));
            }
        });
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        unsafe { System.dealloc(p, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: AllocationProbe = AllocationProbe;

#[test]
fn full_queue_cancel_discards_old_audio_without_allocating() {
    let (producer, mut consumer, control) = pcm_ring();
    for i in 0..POOL_BLOCKS {
        let mut block = producer.acquire().unwrap();
        block.samples_mut().fill(17);
        block
            .prepare(1, 0, (i * BLOCK_FRAMES) as u64, BLOCK_FRAMES)
            .unwrap();
        assert!(producer.submit(block).is_ok());
    }
    assert!(producer.acquire().is_none());
    control.invalidate(2).unwrap();
    let mut output = [9; BLOCK_FRAMES];
    ALLOCS.with(|n| n.set(0));
    MEASURING.with(|m| m.set(true));
    let report = consumer.render(&mut output, 0).unwrap();
    MEASURING.with(|m| m.set(false));
    assert_eq!(ALLOCS.with(|n| n.get()), 0);
    assert!(output.iter().all(|&x| x == 0));
    assert!(report.spans().is_empty());
    let mut recovered = Vec::new();
    while let Some(block) = producer.acquire() {
        recovered.push(block);
    }
    assert_eq!(recovered.len(), POOL_BLOCKS);
    for block in recovered {
        producer.recycle(block);
    }
    let mut late = producer.acquire().unwrap();
    late.prepare(1, 0, 0, 1).unwrap();
    producer.recycle(producer.submit(late).err().unwrap());
}

#[test]
fn pause_retains_audio_and_device_clock_alone_advances_position() {
    let (producer, mut consumer, control) = pcm_ring();
    let mut block = producer.acquire().unwrap();
    block.samples_mut()[..4].copy_from_slice(&[1, 2, 3, 4]);
    block.prepare(1, 3, 100, 4).unwrap();
    assert!(producer.submit(block).is_ok());
    let mut clock = PresentationClock::default();
    clock.reset(1, 0).unwrap();
    control.pause(true);
    let mut output = [0; 4];
    assert_eq!(consumer.render(&mut output, 0).unwrap().silence_frames, 4);
    control.pause(false);
    let report = consumer.render(&mut output, 4).unwrap();
    assert_eq!(output, [1, 2, 3, 4]);
    assert_eq!(clock.position(), None);
    assert_eq!(clock.acknowledge(report.spans()[0], 6).unwrap(), Some(3));
    assert_eq!(clock.position(), Some((3, 102)));
    assert_eq!(clock.acknowledge(report.spans()[0], 8).unwrap(), None);
    assert_eq!(clock.position(), Some((3, 104)));
    let silence = consumer.render(&mut output, 8).unwrap();
    assert!(silence.spans().is_empty());
    assert_eq!(clock.position(), Some((3, 104)));
    clock.reset(2, 12).unwrap();
    assert!(clock.acknowledge(report.spans()[0], 16).is_err());
}

#[test]
fn speed_mapping_uses_source_frames_without_changing_stored_position_units() {
    let mut clock = PresentationClock::default();
    clock.reset(1, 0).unwrap();
    let span = PresentedSpan {
        epoch: 1,
        segment: 0,
        first_source_frame: 0,
        end_source_frame: 1300,
        first_output_frame: 0,
        end_output_frame: 1000,
    };
    clock.acknowledge(span, 500).unwrap();
    assert_eq!(clock.position(), Some((0, 650)));
}

#[test]
fn sustained_pool_reuse_is_allocation_free() {
    let (producer, mut consumer, _control) = pcm_ring();
    let mut out = [0; BLOCK_FRAMES];
    ALLOCS.with(|n| n.set(0));
    MEASURING.with(|m| m.set(true));
    for i in 0..10000u64 {
        let mut block = producer.acquire().unwrap();
        block
            .prepare(1, 0, i * BLOCK_FRAMES as u64, BLOCK_FRAMES)
            .unwrap();
        assert!(producer.submit(block).is_ok());
        consumer.render(&mut out, i * BLOCK_FRAMES as u64).unwrap();
    }
    MEASURING.with(|m| m.set(false));
    assert_eq!(ALLOCS.with(|n| n.get()), 0);
}
