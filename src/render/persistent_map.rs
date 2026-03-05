//! GPU 永続マッピング — map/unmap オーバーヘッド削減
//!
//! フレーム間で永続的にマップされたバッファを管理し、
//! 毎フレームの map/unmap コストを回避する。

/// リングバッファスロット状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotState {
    /// 書き込み可能。
    Free,
    /// GPU が読み取り中。
    InFlight,
    /// 書き込み済み、GPU 送信待ち。
    Ready,
}

/// リングバッファスロット。
#[derive(Debug, Clone)]
pub struct BufferSlot {
    /// スロット内データ。
    pub data: Vec<u8>,
    /// スロット状態。
    pub state: SlotState,
    /// フレーム番号 (デバッグ用)。
    pub frame: u64,
}

/// 永続マッピングバッファ — リングバッファ方式でフレーム間データ転送を管理。
///
/// N スロットのリングバッファを持ち、GPU の読み取りが完了した
/// スロットを再利用することで、map/unmap のレイテンシを隠蔽する。
#[derive(Debug)]
pub struct PersistentBuffer {
    /// スロット配列。
    slots: Vec<BufferSlot>,
    /// 現在の書き込みインデックス。
    write_idx: usize,
    /// スロットサイズ (バイト)。
    slot_size: usize,
    /// 現在のフレーム番号。
    current_frame: u64,
}

impl PersistentBuffer {
    /// 新しい永続バッファを作成。
    ///
    /// `slot_count` はリングバッファのスロット数 (推奨 2–3)。
    /// `slot_size` は各スロットのバイト数。
    #[must_use]
    pub fn new(slot_count: usize, slot_size: usize) -> Self {
        let slot_count = slot_count.max(2);
        let slots = (0..slot_count)
            .map(|_| BufferSlot {
                data: vec![0u8; slot_size],
                state: SlotState::Free,
                frame: 0,
            })
            .collect();

        Self {
            slots,
            write_idx: 0,
            slot_size,
            current_frame: 0,
        }
    }

    /// 次の書き込み可能スロットを取得。
    ///
    /// `Free` 状態のスロットが見つかればそのインデックスを返す。
    /// 全スロットが使用中の場合は `None`。
    #[must_use]
    pub fn acquire_write_slot(&mut self) -> Option<usize> {
        let count = self.slots.len();
        for i in 0..count {
            let idx = (self.write_idx + i) % count;
            if self.slots[idx].state == SlotState::Free {
                self.write_idx = (idx + 1) % count;
                return Some(idx);
            }
        }
        None
    }

    /// スロットにデータを書き込み。
    ///
    /// データがスロットサイズを超える場合は切り詰められる。
    pub fn write_slot(&mut self, slot_idx: usize, data: &[u8]) -> bool {
        if slot_idx >= self.slots.len() {
            return false;
        }
        let slot = &mut self.slots[slot_idx];
        if slot.state != SlotState::Free {
            return false;
        }
        let copy_len = data.len().min(self.slot_size);
        slot.data[..copy_len].copy_from_slice(&data[..copy_len]);
        slot.state = SlotState::Ready;
        slot.frame = self.current_frame;
        true
    }

    /// スロットを GPU 送信中にマーク。
    pub fn mark_in_flight(&mut self, slot_idx: usize) {
        if let Some(slot) = self.slots.get_mut(slot_idx) {
            if slot.state == SlotState::Ready {
                slot.state = SlotState::InFlight;
            }
        }
    }

    /// GPU 処理完了したスロットを解放。
    pub fn release_slot(&mut self, slot_idx: usize) {
        if let Some(slot) = self.slots.get_mut(slot_idx) {
            slot.state = SlotState::Free;
        }
    }

    /// フレーム進行: 完了フレームのスロットを自動解放。
    pub fn advance_frame(&mut self, completed_frame: u64) {
        self.current_frame += 1;
        for slot in &mut self.slots {
            if slot.state == SlotState::InFlight && slot.frame <= completed_frame {
                slot.state = SlotState::Free;
            }
        }
    }

    /// スロット数。
    #[must_use]
    pub const fn slot_count(&self) -> usize {
        self.slots.len()
    }

    /// 空きスロット数。
    #[must_use]
    pub fn free_count(&self) -> usize {
        self.slots
            .iter()
            .filter(|s| s.state == SlotState::Free)
            .count()
    }

    /// スロットサイズ (バイト)。
    #[must_use]
    pub const fn slot_size(&self) -> usize {
        self.slot_size
    }

    /// スロットのデータを読み取り。
    #[must_use]
    pub fn read_slot(&self, slot_idx: usize) -> Option<&[u8]> {
        self.slots.get(slot_idx).map(|s| s.data.as_slice())
    }

    /// スロットの状態を取得。
    #[must_use]
    pub fn slot_state(&self, slot_idx: usize) -> Option<SlotState> {
        self.slots.get(slot_idx).map(|s| s.state)
    }
}

/// Uniform アップロードヘルパー。
///
/// 構造体を永続バッファに書き込むための型安全ラッパー。
#[derive(Debug)]
pub struct UniformUploader {
    buffer: PersistentBuffer,
}

impl UniformUploader {
    /// 新しいアップローダーを作成。
    #[must_use]
    pub fn new(uniform_size: usize, frame_latency: usize) -> Self {
        Self {
            buffer: PersistentBuffer::new(frame_latency + 1, uniform_size),
        }
    }

    /// Uniform データをアップロード。成功時はスロットインデックスを返す。
    pub fn upload(&mut self, data: &[u8]) -> Option<usize> {
        let idx = self.buffer.acquire_write_slot()?;
        if self.buffer.write_slot(idx, data) {
            self.buffer.mark_in_flight(idx);
            Some(idx)
        } else {
            None
        }
    }

    /// フレーム完了通知。
    pub fn frame_complete(&mut self, frame: u64) {
        self.buffer.advance_frame(frame);
    }

    /// 内部バッファへの参照。
    #[must_use]
    pub const fn buffer(&self) -> &PersistentBuffer {
        &self.buffer
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_new() {
        let buf = PersistentBuffer::new(3, 256);
        assert_eq!(buf.slot_count(), 3);
        assert_eq!(buf.slot_size(), 256);
        assert_eq!(buf.free_count(), 3);
    }

    #[test]
    fn buffer_min_slots() {
        let buf = PersistentBuffer::new(1, 64);
        assert_eq!(buf.slot_count(), 2); // 最低2スロット
    }

    #[test]
    fn buffer_acquire_write() {
        let mut buf = PersistentBuffer::new(2, 64);
        let s0 = buf.acquire_write_slot();
        assert!(s0.is_some());
        let s1 = buf.acquire_write_slot();
        assert!(s1.is_some());
        assert_ne!(s0, s1);
    }

    #[test]
    fn buffer_write_and_read() {
        let mut buf = PersistentBuffer::new(2, 4);
        let idx = buf.acquire_write_slot().unwrap();
        assert!(buf.write_slot(idx, &[1, 2, 3, 4]));
        let data = buf.read_slot(idx).unwrap();
        assert_eq!(data, &[1, 2, 3, 4]);
    }

    #[test]
    fn buffer_write_truncates() {
        let mut buf = PersistentBuffer::new(2, 2);
        let idx = buf.acquire_write_slot().unwrap();
        assert!(buf.write_slot(idx, &[1, 2, 3, 4]));
        let data = buf.read_slot(idx).unwrap();
        assert_eq!(&data[..2], &[1, 2]);
    }

    #[test]
    fn buffer_state_transitions() {
        let mut buf = PersistentBuffer::new(2, 4);
        let idx = buf.acquire_write_slot().unwrap();
        assert_eq!(buf.slot_state(idx), Some(SlotState::Free));

        buf.write_slot(idx, &[1, 2, 3, 4]);
        assert_eq!(buf.slot_state(idx), Some(SlotState::Ready));

        buf.mark_in_flight(idx);
        assert_eq!(buf.slot_state(idx), Some(SlotState::InFlight));

        buf.release_slot(idx);
        assert_eq!(buf.slot_state(idx), Some(SlotState::Free));
    }

    #[test]
    fn buffer_no_free_slots() {
        let mut buf = PersistentBuffer::new(2, 4);
        let i0 = buf.acquire_write_slot().unwrap();
        buf.write_slot(i0, &[0; 4]);
        buf.mark_in_flight(i0);

        let i1 = buf.acquire_write_slot().unwrap();
        buf.write_slot(i1, &[0; 4]);
        buf.mark_in_flight(i1);

        assert!(buf.acquire_write_slot().is_none());
        assert_eq!(buf.free_count(), 0);
    }

    #[test]
    fn buffer_advance_frame_releases() {
        let mut buf = PersistentBuffer::new(2, 4);
        let idx = buf.acquire_write_slot().unwrap();
        buf.write_slot(idx, &[0; 4]);
        buf.mark_in_flight(idx);

        assert_eq!(buf.free_count(), 1);
        buf.advance_frame(0); // frame 0 完了 → スロット解放
        assert_eq!(buf.free_count(), 2);
    }

    #[test]
    fn buffer_write_to_non_free_fails() {
        let mut buf = PersistentBuffer::new(2, 4);
        let idx = buf.acquire_write_slot().unwrap();
        buf.write_slot(idx, &[1, 2, 3, 4]);
        // Ready 状態に書き込み試行 → 失敗
        assert!(!buf.write_slot(idx, &[5, 6, 7, 8]));
    }

    #[test]
    fn uploader_basic() {
        let mut up = UniformUploader::new(16, 2);
        let idx = up.upload(&[0u8; 16]);
        assert!(idx.is_some());
    }

    #[test]
    fn uploader_frame_complete() {
        let mut up = UniformUploader::new(16, 1);
        up.upload(&[0u8; 16]);
        up.upload(&[0u8; 16]);
        // 両方 InFlight → 空きなし
        assert!(up.upload(&[0u8; 16]).is_none());
        up.frame_complete(0);
        // frame 0 完了 → スロット解放
        assert!(up.upload(&[0u8; 16]).is_some());
    }

    #[test]
    fn buffer_read_invalid_index() {
        let buf = PersistentBuffer::new(2, 4);
        assert!(buf.read_slot(99).is_none());
        assert!(buf.slot_state(99).is_none());
    }
}
