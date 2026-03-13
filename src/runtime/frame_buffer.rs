use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SlotState {
    Free,
    Writing,
    Ready(u64),
}

#[derive(Debug)]
struct Slot {
    pixels: Vec<u16>,
    state: SlotState,
}

#[derive(Debug)]
struct Inner {
    width: u32,
    height: u32,
    next_seq: u64,
    latest_ready_index: Option<usize>,
    slots: Vec<Slot>,
}

#[derive(Debug, Clone)]
pub struct FrameBufferPool {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Debug)]
pub struct WriteFrameBuffer {
    pool: Arc<Mutex<Inner>>,
    slot_index: usize,
    width: u32,
    height: u32,
    pixels: Option<Vec<u16>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadFrameBuffer {
    pub width: u32,
    pub height: u32,
    pub sequence: u64,
    pub pixels: Vec<u16>,
}

pub trait FrameBufferSource {
    type WriteFrame;
    type ReadFrame;

    fn get_next_frame_buffer(&self) -> Option<Self::WriteFrame>;
    fn publish_frame(&self, write_frame: Self::WriteFrame);
    fn get_latest_frame(&self) -> Option<Self::ReadFrame>;
    fn dimensions(&self) -> (u32, u32);
    fn buffer_count(&self) -> usize;
}

impl FrameBufferPool {
    pub fn new(width: u32, height: u32, buffer_count: usize) -> Self {
        assert!(width > 0, "width must be > 0");
        assert!(height > 0, "height must be > 0");
        assert!(buffer_count >= 2, "buffer_count must be >= 2");
        let len = width as usize * height as usize;
        let mut slots = Vec::with_capacity(buffer_count);
        for _ in 0..buffer_count {
            slots.push(Slot {
                pixels: vec![0; len],
                state: SlotState::Free,
            });
        }
        Self {
            inner: Arc::new(Mutex::new(Inner {
                width,
                height,
                next_seq: 1,
                latest_ready_index: None,
                slots,
            })),
        }
    }

    fn release_write_slot(
        inner: &mut Inner,
        slot_index: usize,
        pixels: Vec<u16>,
        publish: bool,
    ) {
        let slot = &mut inner.slots[slot_index];
        slot.pixels = pixels;
        if publish {
            let seq = inner.next_seq;
            inner.next_seq = inner.next_seq.saturating_add(1);
            slot.state = SlotState::Ready(seq);
            inner.latest_ready_index = Some(slot_index);
        } else {
            slot.state = SlotState::Free;
        }
    }
}

impl WriteFrameBuffer {
    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn pixels_mut(&mut self) -> &mut [u16] {
        self.pixels
            .as_mut()
            .expect("write frame pixels missing")
            .as_mut_slice()
    }
}

impl Drop for WriteFrameBuffer {
    fn drop(&mut self) {
        let Some(pixels) = self.pixels.take() else {
            return;
        };
        if let Ok(mut inner) = self.pool.lock() {
            FrameBufferPool::release_write_slot(&mut inner, self.slot_index, pixels, false);
        }
    }
}

impl FrameBufferSource for FrameBufferPool {
    type WriteFrame = WriteFrameBuffer;
    type ReadFrame = ReadFrameBuffer;

    fn get_next_frame_buffer(&self) -> Option<Self::WriteFrame> {
        let mut inner = self.inner.lock().ok()?;
        let mut selected = None;
        for (idx, slot) in inner.slots.iter_mut().enumerate() {
            if slot.state == SlotState::Free {
                slot.state = SlotState::Writing;
                selected = Some(idx);
                break;
            }
        }
        let slot_index = selected?;
        let pixels = std::mem::take(&mut inner.slots[slot_index].pixels);
        Some(WriteFrameBuffer {
            pool: self.inner.clone(),
            slot_index,
            width: inner.width,
            height: inner.height,
            pixels: Some(pixels),
        })
    }

    fn publish_frame(&self, mut write_frame: Self::WriteFrame) {
        let Some(pixels) = write_frame.pixels.take() else {
            return;
        };
        if let Ok(mut inner) = self.inner.lock() {
            FrameBufferPool::release_write_slot(&mut inner, write_frame.slot_index, pixels, true);
        }
    }

    fn get_latest_frame(&self) -> Option<Self::ReadFrame> {
        let inner = self.inner.lock().ok()?;
        let idx = inner.latest_ready_index?;
        let slot = &inner.slots[idx];
        let sequence = match slot.state {
            SlotState::Ready(seq) => seq,
            _ => return None,
        };
        Some(ReadFrameBuffer {
            width: inner.width,
            height: inner.height,
            sequence,
            pixels: slot.pixels.clone(),
        })
    }

    fn dimensions(&self) -> (u32, u32) {
        if let Ok(inner) = self.inner.lock() {
            (inner.width, inner.height)
        } else {
            (0, 0)
        }
    }

    fn buffer_count(&self) -> usize {
        if let Ok(inner) = self.inner.lock() {
            inner.slots.len()
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FrameBufferPool, FrameBufferSource};

    #[test]
    fn publish_and_read_latest_frame() {
        let pool = FrameBufferPool::new(4, 2, 3);
        let mut write = pool.get_next_frame_buffer().expect("write frame");
        for (i, p) in write.pixels_mut().iter_mut().enumerate() {
            *p = i as u16;
        }
        pool.publish_frame(write);

        let read = pool.get_latest_frame().expect("latest frame");
        assert_eq!(read.width, 4);
        assert_eq!(read.height, 2);
        assert_eq!(read.pixels, vec![0, 1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(read.sequence, 1);
    }

    #[test]
    fn dropped_write_frame_is_discarded() {
        let pool = FrameBufferPool::new(2, 2, 2);
        {
            let mut write = pool.get_next_frame_buffer().expect("write frame");
            write.pixels_mut()[0] = 99;
        }
        assert!(
            pool.get_latest_frame().is_none(),
            "dropped (unpublished) frame should not become latest"
        );
    }
}
